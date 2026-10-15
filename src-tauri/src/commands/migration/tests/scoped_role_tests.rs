//! Same-named roles at sibling org units are distinct roles.

use crate::commands::migration::env::test_env;
use crate::commands::migration::roles::migrate_user_roles;
use crate::commands::migration::summary::MigrationSummary;
use crate::commands::migration::types::IdMapping;
use crate::test_support::{authenticated_client, json_response, not_found, start};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::test::mock_app;

#[tokio::test]
async fn a_role_at_one_destination_customer_is_not_reused_for_a_sibling_customer() {
    // Source: both customers have their own "Admin" (src IDs 5 and 6).
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/1/user-roles") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/10/user-roles") => json_response(json!({
            "data": [{ "roleId": 5, "roleName": "Admin" }]
        })),
        ("GET", "/api/org-units/11/user-roles") => json_response(json!({
            "data": [{ "roleId": 6, "roleName": "Admin" }]
        })),
        _ => not_found(),
    })
    .await;
    // Destination: only the first customer already has an "Admin" (ID 50).
    let mut created_at_second: Option<i64> = None;
    let dest_server = start(move |method, path, _| match (method, path) {
        ("GET", "/api/org-units/2/user-roles") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/200/user-roles") => json_response(json!({
            "data": [{ "roleId": 50, "roleName": "Admin" }]
        })),
        ("GET", "/api/org-units/201/user-roles") => json_response(json!({
            "data": created_at_second
                .map(|id| vec![json!({ "roleId": id, "roleName": "Admin" })])
                .unwrap_or_default()
        })),
        ("POST", "/api/org-units/201/user-roles") => {
            created_at_second = Some(51);
            json_response(json!({ "roleId": 51 }))
        }
        _ => not_found(),
    })
    .await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let app = mock_app();
    let mut mapping = IdMapping::new();
    mapping.org_units.insert(1, 2);
    mapping.customers.insert(10, 200);
    mapping.customers.insert(11, 201);
    let mut summary = MigrationSummary::default();

    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, None, app.handle(), &cancel);
    migrate_user_roles(&env, &mut mapping, &mut summary)
        .await
        .expect("roles migrate");

    assert_eq!(
        dest_server.count("POST", "/api/org-units/200/user-roles"),
        0
    );
    assert_eq!(
        dest_server.count("POST", "/api/org-units/201/user-roles"),
        1,
        "the second customer needs its own Admin role"
    );
    assert_eq!(
        mapping.roles.get(&5),
        Some(&50),
        "first customer reuses its existing role"
    );
    assert_eq!(
        mapping.roles.get(&6),
        Some(&51),
        "second customer maps to the role created for it"
    );
    assert!(
        summary.roles_failed.is_empty(),
        "{:?}",
        summary.roles_failed
    );
}
