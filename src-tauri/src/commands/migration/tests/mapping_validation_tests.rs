//! Role, user, and access-group mappings are validated against what the
//! destination actually lists, and anything that cannot be mapped is reported.

use crate::api::NcSoapClient;
use crate::commands::migration::access_groups::migrate_access_groups;
use crate::commands::migration::env::test_env;
use crate::commands::migration::mapping::{map_group_scope, resolve_destination_role};
use crate::commands::migration::summary::{build_migration_result, MigrationSummary};
use crate::commands::migration::types::IdMapping;
use crate::commands::migration::users::migrate_users;
use crate::test_support::{authenticated_client, json_response, not_found, start};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::test::mock_app;

fn names(pairs: &[(&str, i64)]) -> HashMap<String, i64> {
    pairs
        .iter()
        .map(|(name, id)| (name.to_string(), *id))
        .collect()
}

#[test]
fn a_mapped_role_id_is_only_used_if_the_destination_org_unit_lists_it() {
    let direct = HashMap::from([(1, 701), (2, 999)]);
    let global = names(&[("acme only", 999), ("elsewhere", 555)]);
    let visible = names(&[("shared", 701), ("acme only", 700)]);

    // Direct mapping that the org unit lists: used as is.
    assert_eq!(
        resolve_destination_role(1, Some("shared"), &direct, &global, Some(&visible)),
        Some(701)
    );
    // Direct mapping to a role this org unit cannot see (another customer's): replaced by
    // the same-named role that is visible here, not the global last-wins entry.
    assert_eq!(
        resolve_destination_role(2, Some("acme only"), &direct, &global, Some(&visible)),
        Some(700)
    );
    // Known to the global map only (another org unit): not assigned.
    assert_eq!(
        resolve_destination_role(3, Some("elsewhere"), &direct, &global, Some(&visible)),
        None
    );
    assert_eq!(
        resolve_destination_role(4, None, &direct, &global, Some(&visible)),
        None
    );
    // Org unit could not be listed: fall back to direct, then global names.
    assert_eq!(
        resolve_destination_role(2, None, &direct, &global, None),
        Some(999)
    );
    assert_eq!(
        resolve_destination_role(5, Some("elsewhere"), &direct, &global, None),
        Some(555)
    );
}

#[test]
fn group_scope_keeps_matched_org_units_and_reports_the_rest() {
    let org_units = HashMap::from([(10, 200), (11, 201)]);
    assert_eq!(
        map_group_scope(&[10, 99, 11], &org_units),
        (vec!["200".to_string(), "201".to_string()], vec![99])
    );
    assert_eq!(
        map_group_scope(&[98, 99], &org_units),
        (vec![], vec![98, 99])
    );
    assert_eq!(map_group_scope(&[], &org_units), (vec![], vec![]));
}

#[tokio::test]
async fn users_get_only_roles_valid_at_their_org_unit_and_partial_role_loss_is_reported() {
    let roles = json!({ "data": [
        { "roleId": 1, "roleName": "Shared" },
        { "roleId": 2, "roleName": "Acme Only" },
        { "roleId": 3, "roleName": "Gone" }
    ]});
    let source_server = start(move |method, path, _| match (method, path) {
        ("GET", "/api/org-units/1/user-roles") | ("GET", "/api/org-units/10/user-roles") => {
            json_response(roles.clone())
        }
        ("GET", "/api/org-units/1/users") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/10/users") => json_response(json!({ "data": [{
            "userId": 7, "userName": "pat@example.com", "isEnabled": true,
            "firstName": "Pat", "lastName": "Lee", "email": "pat@example.com",
            "orgUnitId": 10, "roleIds": [1, 2, 3]
        }]})),
        _ => not_found(),
    })
    .await;
    let dest_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/2/user-roles") => json_response(json!({ "data": [
            { "roleId": 701, "roleName": "Shared" }
        ]})),
        ("GET", "/api/org-units/200/user-roles") => json_response(json!({ "data": [
            { "roleId": 701, "roleName": "Shared" },
            { "roleId": 700, "roleName": "Acme Only" }
        ]})),
        ("GET", "/api/org-units/2/users") | ("GET", "/api/org-units/200/users") => {
            json_response(json!({ "data": [] }))
        }
        _ => not_found(),
    })
    .await;
    let soap_server = start(|_, _, _| {
        (
            200,
            "<soap:Envelope><soap:Body><return>900</return></soap:Body></soap:Envelope>"
                .to_string(),
        )
    })
    .await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let soap = NcSoapClient::new(&soap_server.base_url, "test-jwt");
    let app = mock_app();
    let mut mapping = IdMapping::new();
    mapping.org_units.extend([(1, 2), (10, 200)]);
    mapping.customers.insert(10, 200);
    // Role 2 maps directly to an ID that belongs to some other org unit.
    mapping.roles.extend([(1, 701), (2, 999)]);
    mapping.role_names.insert("acme only".into(), 999);
    let mut summary = MigrationSummary::default();

    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, Some(&soap), app.handle(), &cancel);
    migrate_users(&env, &mut mapping, &mut summary)
        .await
        .expect("users migrate");

    let bodies = soap_server.bodies("POST", "/dms2/services2/ServerEI2");
    assert_eq!(bodies.len(), 1, "{:?}", summary.users_failed);
    assert!(
        bodies[0].contains("<ei2:value>200</ei2:value>"),
        "created at the mapped customer"
    );
    assert!(
        bodies[0].contains(
            "<ei2:key>userroleID</ei2:key>\n                <ei2:value>701,700</ei2:value>"
        ),
        "only roles visible at the destination customer are assigned: {}",
        bodies[0]
    );
    assert_eq!(summary.users_no_roles.len(), 1);
    assert_eq!(summary.users_no_roles[0].0, "pat@example.com");
    assert_eq!(summary.users_no_roles[0].1, vec!["gone (ID 3)".to_string()]);
    let result = build_migration_result(&summary, false);
    assert!(
        !result.success,
        "a user that lost a role needs manual follow-up"
    );
    assert!(
        result
            .manual_follow_up
            .iter()
            .any(|item| item.contains("gone (ID 3)")),
        "{:?}",
        result.manual_follow_up
    );
}

#[tokio::test]
async fn access_groups_report_unmapped_scope_and_soap_created_groups() {
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/1/access-groups") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/10/access-groups") => json_response(json!({ "data": [
            { "groupId": 1, "groupName": "Scoped", "groupType": "ORG_UNIT", "orgUnitIds": [10, 99] },
            { "groupId": 2, "groupName": "Orphan", "groupType": "ORG_UNIT", "orgUnitIds": [98, 99] },
            { "groupId": 3, "groupName": "SoapOnly", "groupType": "ORG_UNIT", "orgUnitIds": [10] }
        ]})),
        _ => not_found(),
    })
    .await;
    let mut next_id = 600;
    let dest_server = start(move |method, path, body| match (method, path) {
        ("GET", "/api/org-units/2/access-groups") | ("GET", "/api/org-units/200/access-groups") => {
            json_response(json!({ "data": [] }))
        }
        ("POST", "/api/org-units/200/access-groups") => {
            let payload: Value = serde_json::from_str(body).expect("payload");
            if payload["groupName"] == "SoapOnly" {
                return not_found(); // REST create fails, so the SOAP fallback runs
            }
            next_id += 1;
            json_response(json!({ "groupId": next_id }))
        }
        _ => not_found(),
    })
    .await;
    let soap_server = start(|_, _, _| {
        (
            200,
            "<soap:Envelope><soap:Body><return>77</return></soap:Body></soap:Envelope>".to_string(),
        )
    })
    .await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let soap = NcSoapClient::new(&soap_server.base_url, "test-jwt");
    let app = mock_app();
    let mut mapping = IdMapping::new();
    mapping.org_units.extend([(1, 2), (10, 200)]);
    mapping.customers.insert(10, 200);
    let mut summary = MigrationSummary::default();

    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, Some(&soap), app.handle(), &cancel);
    migrate_access_groups(&env, &mut mapping, &mut summary)
        .await
        .expect("groups migrate");

    let posts: Vec<Value> = dest_server
        .bodies("POST", "/api/org-units/200/access-groups")
        .iter()
        .map(|body| serde_json::from_str(body).expect("payload"))
        .collect();
    assert_eq!(posts.len(), 2, "Orphan is never sent: {:?}", posts);
    assert_eq!(posts[0]["groupName"], "Scoped");
    assert_eq!(
        posts[0]["orgUnitIds"],
        json!(["200"]),
        "unmapped org unit 99 is dropped from the scope"
    );

    assert_eq!(
        summary.access_groups_created,
        vec!["Scoped (at Customer 10)".to_string()]
    );
    assert_eq!(
        summary.access_groups_failed,
        vec!["Orphan (at Customer 10)".to_string()]
    );
    assert_eq!(
        summary.access_groups_created_via_soap,
        vec!["SoapOnly (at Customer 10)".to_string()]
    );

    let result = build_migration_result(&summary, false);
    assert!(!result.success);
    let follow_up = result.manual_follow_up.join("\n");
    assert!(
        follow_up.contains("'Scoped'")
            && follow_up.contains("1 of 2 org unit(s)")
            && follow_up.contains("99"),
        "{}",
        follow_up
    );
    assert!(
        follow_up.contains("Orphan") && follow_up.contains("was not created"),
        "{}",
        follow_up
    );
    assert!(
        follow_up.contains("SoapOnly")
            && follow_up.contains("via SOAP without its members or org-unit scope"),
        "{}",
        follow_up
    );
}
