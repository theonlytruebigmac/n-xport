//! Same-named roles and access groups at different org units.

use crate::import::handlers::access_groups::import_access_group;
use crate::import::handlers::context::ImportContext;
use crate::import::handlers::roles::import_user_role;
use crate::import::handlers::users::import_user;
use crate::import::{AccessGroupImportRow, RowStatus, UserImportRow, UserRoleImportRow};
use crate::test_support::{authenticated_client, json_response, not_found, start, MockServer};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use tauri::test::mock_app;

const SO_ID: i64 = 1;
const ACME: i64 = 10;
const BETA: i64 = 11;

/// Fake SO with two customers. `kind` is "user-roles" or "access-groups";
/// items defined at the SO are visible at every customer, as on N-central.
/// Seeded as (defined-at org unit, id, name).
async fn server(kind: &'static str, seed: Vec<(i64, i64, &'static str)>) -> MockServer {
    let mut items: Vec<(i64, i64, String)> = seed
        .into_iter()
        .map(|(ou, id, name)| (ou, id, name.to_string()))
        .collect();
    let (id_key, name_key) = if kind == "user-roles" {
        ("roleId", "roleName")
    } else {
        ("groupId", "groupName")
    };
    start(move |method, path, body| {
        if method == "GET" && path == "/api/service-orgs/1/customers" {
            return json_response(json!({ "data": [
                { "customerId": ACME, "customerName": "Acme" },
                { "customerId": BETA, "customerName": "Beta" }
            ]}));
        }
        let Some(rest) = path.strip_prefix("/api/org-units/") else {
            return not_found();
        };
        let Some((ou, suffix)) = rest.split_once('/') else {
            return not_found();
        };
        if suffix != kind {
            return not_found();
        }
        let ou: i64 = ou.parse().expect("org unit id");
        match method {
            "GET" => json_response(json!({ "data": items
                .iter()
                .filter(|(defined_at, _, _)| *defined_at == ou || *defined_at == SO_ID)
                .map(|(_, id, name)| json!({ id_key: id, name_key: name }))
                .collect::<Vec<_>>() })),
            "POST" => {
                let payload: Value = serde_json::from_str(body).expect("payload");
                let name = payload[name_key].as_str().expect("name").to_string();
                let id = 800 + items.len() as i64;
                items.push((ou, id, name));
                json_response(json!({ id_key: id }))
            }
            _ => not_found(),
        }
    })
    .await
}

fn role_row(name: &str, customer: Option<&str>) -> UserRoleImportRow {
    serde_json::from_value(json!({ "roleName": name, "customerName": customer })).expect("role row")
}

fn group_row(name: &str, customer: Option<&str>) -> AccessGroupImportRow {
    serde_json::from_value(json!({
        "groupName": name,
        "customerName": customer,
        "orgUnitNames": customer,
    }))
    .expect("group row")
}

#[tokio::test]
async fn a_role_at_one_customer_does_not_hide_the_same_name_at_a_sibling() {
    let server = server(
        "user-roles",
        vec![(ACME, 700, "Admin"), (SO_ID, 701, "Shared")],
    )
    .await;
    let client = authenticated_client(&server).await;
    let app = mock_app();
    let mut ctx = ImportContext::load(&client, SO_ID, &AtomicBool::new(false))
        .await
        .expect("context");

    let app_handle = app.handle().clone();
    macro_rules! run {
        ($row:expr) => {
            import_user_role(2, $row, &mut ctx, &client, None, false, &app_handle)
        };
    }
    let at_acme = run!(role_row("admin", Some("Acme"))).await;
    assert_eq!(at_acme.status, RowStatus::Skipped, "exists at Acme itself");

    let at_beta = run!(role_row("Admin", Some("Beta"))).await;
    assert_eq!(
        at_beta.status,
        RowStatus::Created,
        "Acme's Admin is a different role from Beta's: {}",
        at_beta.message
    );
    assert_eq!(server.count("POST", "/api/org-units/11/user-roles"), 1);

    let repeat = run!(role_row("Admin", Some("Beta"))).await;
    assert_eq!(repeat.status, RowStatus::Skipped, "now exists at Beta");

    let inherited = run!(role_row("Shared", Some("Beta"))).await;
    assert_eq!(
        inherited.status,
        RowStatus::Skipped,
        "inherited from the SO"
    );
    assert!(inherited.message.contains("at or above"));
    assert_eq!(server.count("POST", "/api/org-units/11/user-roles"), 1);
    assert_eq!(server.count("POST", "/api/org-units/10/user-roles"), 0);
}

#[tokio::test]
async fn a_role_created_at_the_so_is_visible_to_every_customer() {
    let server = server("user-roles", vec![]).await;
    let client = authenticated_client(&server).await;
    let app = mock_app();
    let mut ctx = ImportContext::load(&client, SO_ID, &AtomicBool::new(false))
        .await
        .expect("context");

    let created = import_user_role(
        2,
        role_row("Ops", None),
        &mut ctx,
        &client,
        None,
        false,
        app.handle(),
    )
    .await;
    assert_eq!(created.status, RowStatus::Created);
    let below = import_user_role(
        3,
        role_row("Ops", Some("Acme")),
        &mut ctx,
        &client,
        None,
        false,
        app.handle(),
    )
    .await;
    assert_eq!(below.status, RowStatus::Skipped);
    assert_eq!(server.count("POST", "/api/org-units/10/user-roles"), 0);
}

#[tokio::test]
async fn an_access_group_at_one_customer_does_not_hide_the_same_name_at_a_sibling() {
    let server = server("access-groups", vec![(ACME, 600, "Helpdesk")]).await;
    let client = authenticated_client(&server).await;
    let app = mock_app();
    let mut ctx = ImportContext::load(&client, SO_ID, &AtomicBool::new(false))
        .await
        .expect("context");

    let at_acme = import_access_group(
        2,
        group_row("Helpdesk", Some("Acme")),
        &mut ctx,
        &client,
        None,
        false,
        app.handle(),
    )
    .await;
    assert_eq!(at_acme.status, RowStatus::Skipped);
    let at_beta = import_access_group(
        3,
        group_row("Helpdesk", Some("Beta")),
        &mut ctx,
        &client,
        None,
        false,
        app.handle(),
    )
    .await;
    assert_eq!(at_beta.status, RowStatus::Created, "{}", at_beta.message);
    assert_eq!(server.count("POST", "/api/org-units/11/access-groups"), 1);
    let repeat = import_access_group(
        4,
        group_row("helpdesk", Some("Beta")),
        &mut ctx,
        &client,
        None,
        false,
        app.handle(),
    )
    .await;
    assert_eq!(repeat.status, RowStatus::Skipped);
    assert_eq!(server.count("POST", "/api/org-units/11/access-groups"), 1);
}

#[tokio::test]
async fn a_group_created_with_no_response_body_gets_its_real_id_from_the_list() {
    // N-central answers a successful access-group creation with 204 and no body.
    let mut created = false;
    let server = start(move |method, path, _| match (method, path) {
        ("GET", "/api/service-orgs/1/customers") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/1/access-groups") => json_response(json!({
            "data": if created {
                vec![json!({ "groupId": 4242, "orgUnitId": 1, "groupName": "Helpdesk" })]
            } else {
                vec![]
            }
        })),
        ("POST", "/api/org-units/1/access-groups") => {
            created = true;
            (204, String::new())
        }
        _ => not_found(),
    })
    .await;
    let client = authenticated_client(&server).await;
    let app = mock_app();
    let mut ctx = ImportContext::load(&client, SO_ID, &AtomicBool::new(false))
        .await
        .expect("context");

    let first = import_access_group(
        2,
        group_row("Helpdesk", None),
        &mut ctx,
        &client,
        None,
        false,
        app.handle(),
    )
    .await;
    assert_eq!(first.status, RowStatus::Created);
    assert!(
        first.message.contains("4242"),
        "the real ID is reported: {}",
        first.message
    );
    assert_eq!(ctx.group_at(SO_ID, "helpdesk"), Some(4242));

    let repeat = import_access_group(
        3,
        group_row("Helpdesk", None),
        &mut ctx,
        &client,
        None,
        false,
        app.handle(),
    )
    .await;
    assert_eq!(repeat.status, RowStatus::Skipped);
    assert!(
        repeat.message.contains("already exists"),
        "{}",
        repeat.message
    );
    assert_eq!(server.count("POST", "/api/org-units/1/access-groups"), 1);
}

#[tokio::test]
async fn sites_from_other_service_orgs_are_never_a_placement_target() {
    // `/api/sites` lists every site on the server. Site 77 belongs to a customer of a
    // different service org (999), so a user must not be placed there.
    let server = start(|method, path, _| match (method, path) {
        ("GET", "/api/service-orgs/1/customers") => json_response(json!({ "data": [
            { "customerId": ACME, "customerName": "Acme" }
        ]})),
        ("GET", "/api/sites") => json_response(json!({ "data": [
            { "siteId": 5, "siteName": "HQ", "parentId": ACME },
            { "siteId": 77, "siteName": "Other HQ", "parentId": 999 }
        ]})),
        _ => not_found(),
    })
    .await;
    let client = authenticated_client(&server).await;
    let app = mock_app();
    let mut ctx = ImportContext::load(&client, SO_ID, &AtomicBool::new(false))
        .await
        .expect("context");

    assert_eq!(ctx.sites_by_name_flat.get("hq"), Some(&5));
    assert!(!ctx.sites_by_name_flat.contains_key("other hq"));
    assert_eq!(ctx.sites_by_name.len(), 1);

    let user = |customer: &str| -> UserImportRow {
        serde_json::from_value(json!({
            "email": "pat@example.com", "firstName": "Pat", "lastName": "Lee",
            "customerName": customer,
        }))
        .expect("user row")
    };
    let ours = import_user(2, user("HQ"), &mut ctx, None, true, None, app.handle()).await;
    assert_eq!(ours.status, RowStatus::Planned, "{}", ours.message);
    let foreign = import_user(
        3,
        user("Other HQ"),
        &mut ctx,
        None,
        true,
        None,
        app.handle(),
    )
    .await;
    assert_eq!(foreign.status, RowStatus::Error, "{}", foreign.message);
    assert!(
        foreign.message.contains("did not match"),
        "{}",
        foreign.message
    );
}

#[tokio::test]
async fn users_only_resolve_roles_their_own_org_unit_can_see() {
    let server = server(
        "user-roles",
        vec![(ACME, 700, "Acme Only"), (SO_ID, 701, "Shared")],
    )
    .await;
    let client = authenticated_client(&server).await;
    let app = mock_app();
    let mut ctx = ImportContext::load(&client, SO_ID, &AtomicBool::new(false))
        .await
        .expect("context");
    let user = |customer: &str| -> UserImportRow {
        serde_json::from_value(json!({
            "email": "pat@example.com",
            "firstName": "Pat",
            "lastName": "Lee",
            "customerName": customer,
            "roleNames": "Acme Only;Shared",
        }))
        .expect("user row")
    };

    let at_acme = import_user(2, user("Acme"), &mut ctx, None, true, None, app.handle()).await;
    assert!(
        at_acme.message.contains("2 role(s)") && !at_acme.message.contains("unresolved"),
        "{}",
        at_acme.message
    );
    let at_beta = import_user(3, user("Beta"), &mut ctx, None, true, None, app.handle()).await;
    assert!(at_beta.message.contains("1 role(s)"), "{}", at_beta.message);
    assert!(
        at_beta.message.contains("unresolved roles: Acme Only"),
        "{}",
        at_beta.message
    );
}
