//! N-central answers a successful access-group creation with 204 and no body, and its
//! per-org-unit group list omits the org units a group covers.

use crate::commands::migration::access_groups::migrate_access_groups;
use crate::commands::migration::env::test_env;
use crate::commands::migration::summary::MigrationSummary;
use crate::commands::migration::types::IdMapping;
use crate::test_support::{authenticated_client, json_response, not_found, start};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::test::mock_app;

#[tokio::test]
async fn a_group_created_with_no_response_body_is_found_by_name_and_keeps_its_real_scope() {
    // The source list has no orgUnitIds; the detail endpoint does (10 mapped, 99 not).
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/1/access-groups") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/10/access-groups") => json_response(json!({ "data": [
            { "groupId": 1, "orgUnitId": 10, "groupName": "Helpdesk", "groupType": "ORG_UNIT" }
        ]})),
        ("GET", "/api/access-groups/1") => json_response(json!({ "data": {
            "groupId": 1, "orgUnitId": 10, "groupName": "Helpdesk",
            "orgUnitIds": [10, 99], "userIds": [], "autoIncludeNewOrgUnits": false
        }})),
        _ => not_found(),
    })
    .await;
    let mut created = false;
    let dest_server = start(move |method, path, _| match (method, path) {
        ("GET", "/api/org-units/2/access-groups") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/200/access-groups") => json_response(json!({
            "data": if created {
                vec![json!({ "groupId": 777, "orgUnitId": 200, "groupName": "Helpdesk", "groupType": "ORG_UNIT" })]
            } else {
                vec![]
            }
        })),
        ("POST", "/api/org-units/200/access-groups") => {
            created = true;
            (204, String::new())
        }
        _ => not_found(),
    })
    .await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let app = mock_app();
    let mut mapping = IdMapping::new();
    mapping.org_units.extend([(1, 2), (10, 200)]);
    mapping.customers.insert(10, 200);
    let mut summary = MigrationSummary::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, None, app.handle(), &cancel);

    migrate_access_groups(&env, &mut mapping, &mut summary)
        .await
        .expect("groups migrate");

    assert!(
        summary.access_groups_failed.is_empty(),
        "{:?}",
        summary.access_groups_failed
    );
    assert_eq!(
        summary.access_groups_created,
        vec!["Helpdesk (at Customer 10)".to_string()]
    );
    assert_eq!(
        mapping.access_groups.get(&1),
        Some(&777),
        "the ID is looked up by name"
    );
    let posts = dest_server.bodies("POST", "/api/org-units/200/access-groups");
    let payload: Value = serde_json::from_str(&posts[0]).expect("payload");
    assert_eq!(
        payload["orgUnitIds"],
        json!(["200"]),
        "scope comes from the detail endpoint"
    );
    assert_eq!(payload["autoIncludeNewOrgUnits"], json!("false"));
    assert!(
        summary
            .additional_manual_follow_up
            .iter()
            .any(|note| note.contains("1 of 2")),
        "the unmapped org unit is reported: {:?}",
        summary.additional_manual_follow_up
    );
}
