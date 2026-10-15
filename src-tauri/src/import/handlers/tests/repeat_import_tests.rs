//! Repeated and duplicate imports against a stateful fake server.

use crate::api::{NcClient, NcSoapClient};
use crate::import::handlers::context::ImportContext;
use crate::import::handlers::customers::import_customer;
use crate::import::handlers::properties::{import_device_property, import_org_property};
use crate::import::handlers::sites::import_site;
use crate::import::handlers::users::import_user;
use crate::import::{RowOutcome, RowStatus, SiteImportRow, UserImportRow};
use crate::models::*;
use crate::test_support::{authenticated_client, json_response, not_found, start};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use tauri::test::{mock_app, MockRuntime};
use tauri::AppHandle;

const SO_ID: i64 = 1;

fn row<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("test row")
}

fn statuses(outcomes: &[RowOutcome]) -> Vec<RowStatus> {
    outcomes.iter().map(|outcome| outcome.status).collect()
}

async fn load_context(client: &NcClient) -> ImportContext {
    ImportContext::load(client, SO_ID, &AtomicBool::new(false))
        .await
        .expect("context loads from the fake server")
}

async fn import_customers(
    names: &[&str],
    ctx: &mut ImportContext,
    client: &NcClient,
    app: &AppHandle<MockRuntime>,
) -> Vec<RowOutcome> {
    let mut outcomes = Vec::new();
    for (index, name) in names.iter().enumerate() {
        outcomes.push(
            import_customer(
                index + 2,
                row(serde_json::json!({ "customerName": name })),
                ctx,
                client,
                None,
                false,
                app,
            )
            .await,
        );
    }
    outcomes
}

/// Fake SO that remembers customers created through POST. When
/// `return_ids` is false the create response carries no ID.
async fn customer_server(return_ids: bool) -> crate::test_support::MockServer {
    let mut customers: Vec<(i64, String)> = Vec::new();
    let customers_path = format!("/api/service-orgs/{}/customers", SO_ID);
    start(move |method, path, body| {
        if path != customers_path {
            return not_found();
        }
        match method {
            "GET" => json_response(serde_json::json!({
                "data": customers
                    .iter()
                    .map(|(id, name)| serde_json::json!({
                        "customerId": id,
                        "customerName": name,
                    }))
                    .collect::<Vec<_>>()
            })),
            "POST" => {
                let payload: Value = serde_json::from_str(body).expect("customer payload");
                let id = 100 + customers.len() as i64;
                customers.push((id, payload["customerName"].as_str().unwrap().to_string()));
                json_response(if return_ids {
                    serde_json::json!({ "customerId": id })
                } else {
                    serde_json::json!({})
                })
            }
            _ => not_found(),
        }
    })
    .await
}

#[tokio::test]
async fn repeating_a_customer_import_creates_each_customer_once() {
    let server = customer_server(true).await;
    let client = authenticated_client(&server).await;
    let app = mock_app();
    let post_path = format!("/api/service-orgs/{}/customers", SO_ID);

    // First run, with a duplicate row (different case) inside the file.
    let mut ctx = load_context(&client).await;
    let first = import_customers(&["Acme", "ACME", "Beta"], &mut ctx, &client, app.handle()).await;
    assert_eq!(
        statuses(&first),
        vec![RowStatus::Created, RowStatus::Skipped, RowStatus::Created]
    );
    assert_eq!(server.count("POST", &post_path), 2);

    // Same CSV again in the same session (cached context is reused).
    let second = import_customers(&["Acme", "ACME", "Beta"], &mut ctx, &client, app.handle()).await;
    assert!(statuses(&second)
        .iter()
        .all(|status| *status == RowStatus::Skipped));

    // Same CSV again after the app restarts: the context is rebuilt from the server.
    let mut reloaded = load_context(&client).await;
    let third = import_customers(&["Acme", "Beta"], &mut reloaded, &client, app.handle()).await;
    assert!(statuses(&third)
        .iter()
        .all(|status| *status == RowStatus::Skipped));
    assert!(third[0].message.contains("already exists"));
    assert_eq!(server.count("POST", &post_path), 2, "no run may POST again");
}

#[tokio::test]
async fn customer_created_without_a_returned_id_is_not_created_twice() {
    let server = customer_server(false).await;
    let client = authenticated_client(&server).await;
    let app = mock_app();

    let mut ctx = load_context(&client).await;
    let outcomes = import_customers(&["Acme", "acme"], &mut ctx, &client, app.handle()).await;

    assert_eq!(
        statuses(&outcomes),
        vec![RowStatus::Created, RowStatus::Skipped]
    );
    assert!(outcomes[1].message.contains("earlier in this session"));
    assert_eq!(
        server.count("POST", &format!("/api/service-orgs/{}/customers", SO_ID)),
        1
    );
}

#[tokio::test]
async fn repeating_a_site_import_creates_each_site_once() {
    let mut sites: Vec<(i64, String)> = Vec::new();
    let server = start(move |method, path, body| match (method, path) {
        ("GET", "/api/service-orgs/1/customers") => json_response(serde_json::json!({
            "data": [{ "customerId": 10, "customerName": "Acme" }]
        })),
        ("GET", "/api/sites") => json_response(serde_json::json!({
            "data": sites
                .iter()
                .map(|(id, name)| serde_json::json!({
                    "siteId": id,
                    "siteName": name,
                    "parentId": 10,
                }))
                .collect::<Vec<_>>()
        })),
        ("POST", "/api/customers/10/sites") => {
            let payload: Value = serde_json::from_str(body).expect("site payload");
            let id = 500 + sites.len() as i64;
            sites.push((id, payload["siteName"].as_str().unwrap().to_string()));
            json_response(serde_json::json!({ "siteId": id }))
        }
        _ => not_found(),
    })
    .await;
    let client = authenticated_client(&server).await;
    let app = mock_app();

    let site_rows = || {
        ["HQ", "hq", "Branch"].map(|site| {
            row::<SiteImportRow>(serde_json::json!({ "customerName": "Acme", "siteName": site }))
        })
    };
    let run = |mut ctx: ImportContext| {
        let client = client.clone();
        let handle = app.handle().clone();
        async move {
            let mut outcomes = Vec::new();
            for (index, site) in site_rows().into_iter().enumerate() {
                outcomes
                    .push(import_site(index + 2, site, &mut ctx, &client, false, &handle).await);
            }
            outcomes
        }
    };

    let first = run(load_context(&client).await).await;
    assert_eq!(
        statuses(&first),
        vec![RowStatus::Created, RowStatus::Skipped, RowStatus::Created]
    );
    let second = run(load_context(&client).await).await;
    assert!(statuses(&second)
        .iter()
        .all(|status| *status == RowStatus::Skipped));
    assert_eq!(server.count("POST", "/api/customers/10/sites"), 2);
}

#[tokio::test]
async fn repeated_org_property_rows_are_safe_repeat_writes_in_file_order() {
    let server = start(|method, path, _| match (method, path) {
        ("GET", "/api/service-orgs/1/customers") => {
            json_response(serde_json::json!({ "data": [] }))
        }
        ("GET", "/api/org-units/1/custom-properties") => json_response(serde_json::json!({
            "data": [{
                "propertyId": 5,
                "propertyName": "Asset Tag",
                "value": "old",
                "propertyType": "TEXT"
            }]
        })),
        ("PUT", "/api/org-units/1/custom-properties/5") => json_response(serde_json::json!({})),
        _ => not_found(),
    })
    .await;
    let client = authenticated_client(&server).await;
    let app = mock_app();
    let cancel = AtomicBool::new(false);
    let mut ctx = ImportContext::load(&client, SO_ID, &cancel)
        .await
        .expect("context");

    let mut outcomes = Vec::new();
    for (index, value) in ["A1", "A1", "A2"].iter().enumerate() {
        outcomes.push(
            import_org_property(
                index + 2,
                row(serde_json::json!({ "propertyName": "asset tag", "value": value })),
                &mut ctx,
                &client,
                false,
                &cancel,
                app.handle(),
            )
            .await,
        );
    }

    assert!(statuses(&outcomes)
        .iter()
        .all(|status| *status == RowStatus::Updated));
    let bodies: Vec<Value> = server
        .bodies("PUT", "/api/org-units/1/custom-properties/5")
        .iter()
        .map(|body| serde_json::from_str(body).expect("put body"))
        .collect();
    assert_eq!(
        bodies
            .iter()
            .map(|body| body["value"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["A1", "A1", "A2"],
        "each row is applied in order, so the last row wins"
    );
    assert_eq!(
        server.count("GET", "/api/org-units/1/custom-properties"),
        1,
        "property definitions are fetched once and reused"
    );

    // A dry run after live writes plans the same rows and writes nothing.
    let dry = import_org_property(
        2,
        row(serde_json::json!({ "propertyName": "Asset Tag", "value": "A2" })),
        &mut ctx,
        &client,
        true,
        &cancel,
        app.handle(),
    )
    .await;
    assert_eq!(dry.status, RowStatus::Planned);
    assert_eq!(
        server.count("PUT", "/api/org-units/1/custom-properties/5"),
        3
    );
}

#[tokio::test]
async fn repeated_device_property_rows_write_to_the_one_matching_device() {
    let server = start(|method, path, _| match (method, path) {
        ("GET", "/api/service-orgs/1/customers") => json_response(serde_json::json!({
            "data": [{ "customerId": 10, "customerName": "Acme" }]
        })),
        ("GET", "/api/org-units/10/devices") => json_response(serde_json::json!({
            "data": [
                { "deviceId": 77, "longName": "SRV-01", "customerId": 10 },
                { "deviceId": 78, "longName": "SRV-02", "customerId": 10 }
            ]
        })),
        ("GET", "/api/devices/77/custom-properties") => json_response(serde_json::json!({
            "data": [{ "propertyId": 9, "propertyName": "Owner", "propertyType": "TEXT" }]
        })),
        ("PUT", "/api/devices/77/custom-properties/9") => json_response(serde_json::json!({})),
        _ => not_found(),
    })
    .await;
    let client = authenticated_client(&server).await;
    let app = mock_app();
    let cancel = AtomicBool::new(false);
    let mut ctx = ImportContext::load(&client, SO_ID, &cancel)
        .await
        .expect("context");

    let mut outcomes = Vec::new();
    for index in 0..2 {
        outcomes.push(
            import_device_property(
                index + 2,
                row(serde_json::json!({
                    "customerName": "Acme",
                    "deviceName": "srv-01",
                    "propertyName": "Owner",
                    "value": "ops"
                })),
                &mut ctx,
                &client,
                false,
                &cancel,
                app.handle(),
            )
            .await,
        );
    }

    assert!(statuses(&outcomes)
        .iter()
        .all(|status| *status == RowStatus::Updated));
    assert_eq!(
        server.count("PUT", "/api/devices/77/custom-properties/9"),
        2
    );
    assert_eq!(
        server.count("PUT", "/api/devices/78/custom-properties/9"),
        0
    );
    assert_eq!(server.count("GET", "/api/org-units/10/devices"), 1);
    assert_eq!(server.count("GET", "/api/devices/77/custom-properties"), 1);
}

fn soap_response(id: i64) -> (u16, String) {
    (
        200,
        format!(
            r#"<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body><ns1:userAddResponse><return>{}</return></ns1:userAddResponse></soap:Body></soap:Envelope>"#,
            id
        ),
    )
}

#[tokio::test]
async fn repeated_user_rows_call_soap_once_and_server_refusals_are_skipped() {
    // The server creates a user the first time and refuses (-1) afterwards,
    // as N-central does for an existing login.
    let mut created = std::collections::HashSet::new();
    let server = start(move |method, path, body| {
        if method == "POST" && path == "/dms2/services2/ServerEI2" {
            let login = body
                .split("<ei2:username>")
                .nth(1)
                .and_then(|rest| rest.split("</ei2:username>").next())
                .unwrap_or_default()
                .to_string();
            let email_known = body.contains("ana@example.com");
            let id = if email_known && created.insert("ana".to_string()) {
                900
            } else if !email_known && created.insert(login) {
                901
            } else {
                -1
            };
            return soap_response(id);
        }
        not_found()
    })
    .await;
    let soap = NcSoapClient::new(&server.base_url, "test-jwt");
    let app = mock_app();
    let user = |email: &str| -> UserImportRow {
        row(serde_json::json!({
            "email": email,
            "firstName": "Test",
            "lastName": "User",
            "customerName": "SO"
        }))
    };
    let soap_path = "/dms2/services2/ServerEI2";

    let mut ctx = empty_context();

    let first = import_user(
        2,
        user("ana@example.com"),
        &mut ctx,
        Some(&soap),
        false,
        None,
        app.handle(),
    )
    .await;
    let repeated_in_file = import_user(
        3,
        user("ANA@example.com"),
        &mut ctx,
        Some(&soap),
        false,
        None,
        app.handle(),
    )
    .await;
    assert_eq!(first.status, RowStatus::Created);
    assert_eq!(repeated_in_file.status, RowStatus::Skipped);
    assert!(repeated_in_file.message.contains("earlier in this session"));
    assert_eq!(
        server.count("POST", soap_path),
        1,
        "repeat row must not reach SOAP"
    );

    // After a restart the context forgets the login; the server's -1 is a skip, not an error.
    let mut fresh = empty_context();
    let rerun = import_user(
        2,
        user("ana@example.com"),
        &mut fresh,
        Some(&soap),
        false,
        None,
        app.handle(),
    )
    .await;
    assert_eq!(rerun.status, RowStatus::Skipped);
    assert_eq!(server.count("POST", soap_path), 2);
}

fn empty_context() -> ImportContext {
    ImportContext {
        source_so_id: SO_ID,
        source_so_name: None,
        customers_by_name: HashMap::new(),
        sites_by_name: HashMap::new(),
        sites_by_name_flat: HashMap::new(),
        roles_by_name: HashMap::new(),
        groups_by_name: HashMap::new(),
        roles_by_ou: HashMap::new(),
        groups_by_ou: HashMap::new(),
        permission_lookup: PermissionLookup::from_csv(""),
        org_properties_by_ou: HashMap::new(),
        devices_by_ou: HashMap::new(),
        device_properties_by_id: HashMap::new(),
        unconfirmed_creates: HashSet::new(),
        created_users: HashSet::new(),
    }
}
