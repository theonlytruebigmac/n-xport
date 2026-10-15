//! SOAP is used by imports only as a fallback or where it is required.

use crate::api::NcSoapClient;
use crate::import::handlers::context::ImportContext;
use crate::import::handlers::customers::import_customer;
use crate::import::handlers::users::import_user;
use crate::import::{CustomerImportRow, RowStatus, UserImportRow};
use crate::models::*;
use crate::test_support::{authenticated_client, json_response, not_found, start, MockServer};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tauri::test::mock_app;

const SOAP_PATH: &str = "/dms2/services2/ServerEI2";

async fn soap_disabled_server() -> MockServer {
    start(|_, _, _| {
        (
            500,
            "<soap:Envelope><soap:Body><soap:Fault><faultcode>soap:Server</faultcode>\
             <faultstring>SOAP API is disabled</faultstring></soap:Fault></soap:Body></soap:Envelope>"
                .to_string(),
        )
    })
    .await
}

fn customer(name: &str) -> CustomerImportRow {
    serde_json::from_value(json!({ "customerName": name })).expect("customer row")
}

fn context() -> ImportContext {
    ImportContext {
        source_so_id: 1,
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

#[tokio::test]
async fn customer_import_uses_soap_only_when_the_rest_create_fails() {
    let soap_server = soap_disabled_server().await;
    let soap = NcSoapClient::new(&soap_server.base_url, "test-jwt");
    let app = mock_app();

    let rest_ok = start(|method, path, _| match (method, path) {
        ("POST", "/api/service-orgs/1/customers") => json_response(json!({ "customerId": 5 })),
        _ => not_found(),
    })
    .await;
    let client = authenticated_client(&rest_ok).await;
    let outcome = import_customer(
        2,
        customer("Acme"),
        &mut context(),
        &client,
        Some(&soap),
        false,
        app.handle(),
    )
    .await;
    assert_eq!(outcome.status, RowStatus::Created);
    assert_eq!(soap_server.count("POST", SOAP_PATH), 0, "REST succeeded");

    let rest_down = start(|_, _, _| not_found()).await;
    let client = authenticated_client(&rest_down).await;
    let outcome = import_customer(
        2,
        customer("Acme"),
        &mut context(),
        &client,
        Some(&soap),
        false,
        app.handle(),
    )
    .await;
    assert_eq!(
        soap_server.count("POST", SOAP_PATH),
        1,
        "SOAP is the fallback"
    );
    assert_eq!(outcome.status, RowStatus::Error, "{}", outcome.message);
    assert!(
        outcome.message.contains("REST") && outcome.message.contains("SOAP"),
        "{}",
        outcome.message
    );

    let outcome = import_customer(
        2,
        customer("Acme"),
        &mut context(),
        &client,
        None,
        false,
        app.handle(),
    )
    .await;
    assert_eq!(outcome.status, RowStatus::Error);
}

#[tokio::test]
async fn user_import_reports_an_error_when_soap_is_missing_or_disabled() {
    let app = mock_app();
    let user: UserImportRow = serde_json::from_value(json!({
        "email": "pat@example.com", "firstName": "Pat", "lastName": "Lee", "customerName": "SO"
    }))
    .expect("user row");

    let outcome = import_user(
        2,
        user.clone(),
        &mut context(),
        None,
        false,
        None,
        app.handle(),
    )
    .await;
    assert_eq!(outcome.status, RowStatus::Error);
    assert!(
        outcome.message.contains("requires SOAP"),
        "{}",
        outcome.message
    );

    let soap_server = soap_disabled_server().await;
    let soap = NcSoapClient::new(&soap_server.base_url, "test-jwt");
    let outcome = import_user(
        2,
        user.clone(),
        &mut context(),
        Some(&soap),
        false,
        None,
        app.handle(),
    )
    .await;
    assert_eq!(outcome.status, RowStatus::Error, "{}", outcome.message);
    assert_eq!(soap_server.count("POST", SOAP_PATH), 1);

    // A dry run never contacts SOAP.
    let outcome = import_user(
        2,
        user,
        &mut context(),
        Some(&soap),
        true,
        None,
        app.handle(),
    )
    .await;
    assert_eq!(outcome.status, RowStatus::Planned);
    assert_eq!(soap_server.count("POST", SOAP_PATH), 1);
}
