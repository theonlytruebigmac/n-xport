//! A full role, user, and access-group migration, run twice against a destination that keeps
//! state. The second run must create nothing and must not be reported as needing follow-up.

use crate::api::NcSoapClient;
use crate::commands::migration::env::test_env;
use crate::commands::migration::run::run_migration;
use crate::commands::migration::types::MigrationOptions;
use crate::test_support::{authenticated_client, json_response, not_found, start};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;
use tauri::test::mock_app;

/// What the fake destination currently holds.
#[derive(Default)]
struct Dest {
    /// (org unit the role is defined at, id, name)
    roles: Vec<(i64, i64, String)>,
    /// (org unit the group is defined at, id, name, member user ids)
    groups: Vec<(i64, i64, String, Vec<i64>)>,
    /// (login, org unit, id)
    users: Vec<(String, i64, i64)>,
    next_id: i64,
}

impl Dest {
    fn new_id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }
}

/// The value of the setting named `key` in a SOAP key/value envelope.
fn soap_setting(body: &str, key: &str) -> String {
    let marker = format!("<ei2:key>{}</ei2:key>", key);
    let after = &body[body.find(&marker).expect("setting present") + marker.len()..];
    let start = after.find("<ei2:value>").expect("value") + "<ei2:value>".len();
    after[start
        ..after[start..]
            .find("</ei2:value>")
            .map(|e| e + start)
            .expect("end")]
        .to_string()
}

#[tokio::test]
async fn repeating_a_role_user_and_group_migration_creates_and_flags_nothing() {
    // Source service org 1 with one customer (10): roles, a user at the customer holding
    // both roles and both groups, and an SO-level and a customer-level group.
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/service-orgs/1/customers") => json_response(json!({ "data": [
            { "customerId": 10, "customerName": "Acme" }
        ]})),
        ("GET", "/api/customers/10/sites") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/1/user-roles") => json_response(json!({ "data": [
            { "roleId": 1, "roleName": "Shared" }
        ]})),
        ("GET", "/api/org-units/10/user-roles") => json_response(json!({ "data": [
            { "roleId": 1, "roleName": "Shared" },
            { "roleId": 2, "roleName": "Acme Only" }
        ]})),
        ("GET", "/api/org-units/1/users") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/10/users") => json_response(json!({ "data": [{
            "userId": 7, "userName": "pat@example.com", "isEnabled": true,
            "firstName": "Pat", "lastName": "Lee", "email": "pat@example.com",
            "orgUnitId": 10, "roleIds": [1, 2], "accessGroupIds": [11, 12]
        }]})),
        ("GET", "/api/org-units/1/access-groups") => json_response(json!({ "data": [
            { "groupId": 11, "orgUnitId": 1, "groupName": "All Hands", "groupType": "ORG_UNIT" }
        ]})),
        ("GET", "/api/org-units/10/access-groups") => json_response(json!({ "data": [
            { "groupId": 11, "orgUnitId": 1, "groupName": "All Hands", "groupType": "ORG_UNIT" },
            { "groupId": 12, "orgUnitId": 10, "groupName": "Acme Team", "groupType": "ORG_UNIT" }
        ]})),
        ("GET", "/api/access-groups/11") => json_response(json!({ "data": {
            "groupId": 11, "orgUnitId": 1, "groupName": "All Hands",
            "orgUnitIds": [1, 10], "userIds": [7], "autoIncludeNewOrgUnits": true
        }})),
        ("GET", "/api/access-groups/12") => json_response(json!({ "data": {
            "groupId": 12, "orgUnitId": 10, "groupName": "Acme Team",
            "orgUnitIds": [10], "userIds": [7], "autoIncludeNewOrgUnits": false
        }})),
        _ => not_found(),
    })
    .await;

    // Destination service org 2 with the customer already present (200), and no roles,
    // users, or groups yet. REST and SOAP share one state, as on a real server.
    let state = Arc::new(Mutex::new(Dest {
        next_id: 500,
        ..Default::default()
    }));
    let rest_state = state.clone();
    let dest_server = start(move |method, path, body| {
        let mut dest = rest_state.lock().unwrap();
        let payload: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let ou_of = |path: &str, suffix: &str| -> Option<i64> {
            path.strip_prefix("/api/org-units/")?.strip_suffix(suffix)?.trim_end_matches('/').parse().ok()
        };
        // A role or group defined at the service org (2) is visible at every customer.
        let visible = |defined_at: i64, ou: i64| defined_at == ou || defined_at == 2;
        match method {
            "GET" => {
                if path == "/api/service-orgs/2/customers" {
                    return json_response(json!({ "data": [{ "customerId": 200, "customerName": "Acme" }] }));
                }
                if path == "/api/customers/200/sites" {
                    return json_response(json!({ "data": [] }));
                }
                if let Some(ou) = ou_of(path, "/user-roles") {
                    let roles: Vec<Value> = dest.roles.iter().filter(|r| visible(r.0, ou))
                        .map(|r| json!({ "roleId": r.1, "roleName": r.2 })).collect();
                    return json_response(json!({ "data": roles }));
                }
                if let Some(ou) = ou_of(path, "/access-groups") {
                    let groups: Vec<Value> = dest.groups.iter().filter(|g| visible(g.0, ou))
                        .map(|g| json!({ "groupId": g.1, "orgUnitId": g.0, "groupName": g.2, "groupType": "ORG_UNIT" })).collect();
                    return json_response(json!({ "data": groups }));
                }
                if ou_of(path, "/users").is_some() {
                    let users: Vec<Value> = dest.users.iter()
                        .map(|u| json!({ "userId": u.2, "userName": u.0, "isEnabled": true, "orgUnitId": u.1 })).collect();
                    return json_response(json!({ "data": users }));
                }
                if let Some(id) = path.strip_prefix("/api/access-groups/").and_then(|id| id.parse::<i64>().ok()) {
                    return match dest.groups.iter().find(|g| g.1 == id) {
                        Some(g) => json_response(json!({ "data": {
                            "groupId": g.1, "orgUnitId": g.0, "groupName": g.2,
                            "orgUnitIds": [g.0], "userIds": g.3
                        }})),
                        None => not_found(),
                    };
                }
                not_found()
            }
            "POST" => {
                if let Some(ou) = ou_of(path, "/user-roles") {
                    let id = dest.new_id();
                    dest.roles.push((ou, id, payload["roleName"].as_str().unwrap().into()));
                    return json_response(json!({ "roleId": id }));
                }
                if let Some(ou) = ou_of(path, "/access-groups") {
                    let id = dest.new_id();
                    let members: Vec<i64> = payload["userIds"].as_array().unwrap().iter()
                        .map(|v| v.as_str().unwrap().parse().unwrap()).collect();
                    dest.groups.push((ou, id, payload["groupName"].as_str().unwrap().into(), members));
                    return (204, String::new());
                }
                not_found()
            }
            _ => not_found(),
        }
    })
    .await;
    let soap_state = state.clone();
    let soap_server = start(move |_, _, body| {
        let mut dest = soap_state.lock().unwrap();
        if body.contains("<ei2:userAdd>") {
            let id = dest.new_id();
            let login = soap_setting(body, "username");
            let customer: i64 = soap_setting(body, "customerID").parse().unwrap();
            dest.users.push((login, customer, id));
            return (200, format!("<soap:Envelope><soap:Body><return>{}</return></soap:Body></soap:Envelope>", id));
        }
        if body.contains("<ei2:userRoleAdd>") {
            let id = dest.new_id();
            let customer: i64 = soap_setting(body, "customerID").parse().unwrap();
            dest.roles.push((customer, id, soap_setting(body, "roleName")));
            return (200, format!("<soap:Envelope><soap:Body><return>{}</return></soap:Body></soap:Envelope>", id));
        }
        (500, "<soap:Fault><faultcode>x</faultcode><faultstring>unexpected</faultstring></soap:Fault>".into())
    })
    .await;

    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let soap = NcSoapClient::new(&soap_server.base_url, "test-jwt");
    let app = mock_app();
    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, Some(&soap), app.handle(), &cancel);
    let options = MigrationOptions {
        customers: false,
        user_roles: true,
        users: true,
        access_groups: true,
        org_properties: false,
        device_properties: false,
    };

    let first = run_migration(&env, &options).await.expect("first run");
    assert!(first.issues.is_empty(), "{:?}", first.issues);
    let (roles, users, groups) = {
        let dest = state.lock().unwrap();
        (dest.roles.len(), dest.users.len(), dest.groups.len())
    };
    assert_eq!(
        (roles, users, groups),
        (2, 1, 2),
        "each source role, user, and group is created once"
    );
    {
        let dest = state.lock().unwrap();
        let user_id = dest.users[0].2;
        assert_eq!(
            dest.users[0].1, 200,
            "the user is created at the mapped customer"
        );
        assert!(
            dest.groups.iter().all(|g| g.3 == vec![user_id]),
            "groups created in the same run include the migrated user: {:?}",
            dest.groups
        );
    }
    let soap_calls_after_first = soap_server.count("POST", "/dms2/services2/ServerEI2");

    let second = run_migration(&env, &options).await.expect("repeat run");
    assert!(second.issues.is_empty(), "{:?}", second.issues);
    {
        let dest = state.lock().unwrap();
        assert_eq!(
            (dest.roles.len(), dest.users.len(), dest.groups.len()),
            (2, 1, 2),
            "the repeat creates nothing"
        );
    }
    assert_eq!(
        soap_server.count("POST", "/dms2/services2/ServerEI2"),
        soap_calls_after_first,
        "the repeat makes no further SOAP calls"
    );
    assert!(
        second.manual_follow_up.is_empty(),
        "every migrated user is already in every group, so nothing needs follow-up: {:?}",
        second.manual_follow_up
    );
    assert!(second.success, "{}", second.message);
    assert!(
        !second.skipped.is_empty(),
        "existing items are reported as skipped"
    );
}
