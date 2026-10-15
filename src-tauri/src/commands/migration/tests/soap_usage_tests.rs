//! SOAP is attempted only where it is required or as a fallback, and a missing
//! or failing SOAP API surfaces as a failed migration rather than a success.

use super::repeat_migration_tests::{dest_server, source_server};
use crate::api::NcSoapClient;
use crate::commands::migration::customers::migrate_customers_and_sites;
use crate::commands::migration::env::test_env;
use crate::commands::migration::properties::migrate_org_properties;
use crate::commands::migration::summary::{build_migration_result, MigrationSummary};
use crate::commands::migration::types::IdMapping;
use crate::commands::migration::users::migrate_users;
use crate::test_support::{authenticated_client, json_response, not_found, start, MockServer};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::test::mock_app;

const SOAP_PATH: &str = "/dms2/services2/ServerEI2";

/// A SOAP endpoint that rejects every call, as a server with SOAP disabled would.
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

#[tokio::test]
async fn customer_and_site_migration_over_rest_never_calls_soap() {
    let source_server = source_server().await;
    let dest_server = dest_server().await;
    let soap_server = soap_disabled_server().await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let soap = NcSoapClient::new(&soap_server.base_url, "test-jwt");
    let app = mock_app();
    let mut mapping = IdMapping::new();
    let mut summary = MigrationSummary::default();

    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, Some(&soap), app.handle(), &cancel);
    migrate_customers_and_sites(&env, &mut mapping, &mut summary)
        .await
        .expect("migrates");

    assert!(summary.issues().is_empty(), "{:?}", summary.issues());
    assert!(
        soap_server.requests().is_empty(),
        "REST succeeded, so SOAP must not be called"
    );
}

async fn property_servers(put_status: u16) -> (MockServer, MockServer) {
    // Properties are listed per org unit and carry no org-unit ID, as on a live server.
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/1/custom-properties") => json_response(json!({
            "data": [{ "propertyId": 5, "propertyName": "Tag", "value": "x" }]
        })),
        _ => not_found(),
    })
    .await;
    let dest_server = start(move |method, path, _| match (method, path) {
        ("GET", "/api/org-units/2/custom-properties") => json_response(json!({
            "data": [{ "propertyId": 50, "propertyName": "Tag", "value": "" }]
        })),
        ("PUT", "/api/org-units/2/custom-properties/50") => (put_status, json!({}).to_string()),
        _ => not_found(),
    })
    .await;
    (source_server, dest_server)
}

async fn run_org_properties(
    source_server: &MockServer,
    dest_server: &MockServer,
    soap: Option<&NcSoapClient>,
) -> MigrationSummary {
    let source = authenticated_client(source_server).await;
    let dest = authenticated_client(dest_server).await;
    let app = mock_app();
    let mapping = IdMapping::new();
    let mut summary = MigrationSummary::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, soap, app.handle(), &cancel);
    migrate_org_properties(&env, &mapping, &mut summary)
        .await
        .expect("runs");
    summary
}

#[tokio::test]
async fn property_migration_calls_soap_only_after_a_rest_failure() {
    let soap_server = soap_disabled_server().await;
    let soap = NcSoapClient::new(&soap_server.base_url, "test-jwt");

    let (source_server, dest_server) = property_servers(200).await;
    let summary = run_org_properties(&source_server, &dest_server, Some(&soap)).await;
    assert!(summary.issues().is_empty(), "{:?}", summary.issues());
    assert_eq!(
        dest_server.count("PUT", "/api/org-units/2/custom-properties/50"),
        1
    );
    assert_eq!(
        soap_server.count("POST", SOAP_PATH),
        0,
        "REST write succeeded"
    );

    let (source_server, dest_server) = property_servers(404).await;
    let summary = run_org_properties(&source_server, &dest_server, Some(&soap)).await;
    assert_eq!(
        soap_server.count("POST", SOAP_PATH),
        1,
        "SOAP is the fallback for a failed REST write"
    );
    assert_eq!(
        summary.properties_failed.len(),
        1,
        "SOAP being disabled leaves the property failed"
    );
    assert!(
        summary.properties_failed[0].contains("SOAP"),
        "{:?}",
        summary.properties_failed
    );

    let (source_server, dest_server) = property_servers(404).await;
    let summary = run_org_properties(&source_server, &dest_server, None).await;
    assert_eq!(
        summary.properties_failed.len(),
        1,
        "no SOAP client: failure is reported, not hidden"
    );
    assert!(!build_migration_result(&summary, false).success);
}

async fn run_users(soap: Option<&NcSoapClient>) -> (MigrationSummary, MockServer) {
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/1/user-roles") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/1/users") => json_response(json!({
            "data": [{
                "userId": 7, "userName": "pat@example.com", "isEnabled": true,
                "firstName": "Pat", "lastName": "Lee", "email": "pat@example.com"
            }]
        })),
        _ => not_found(),
    })
    .await;
    let dest_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/2/user-roles") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/2/users") => json_response(json!({ "data": [] })),
        _ => not_found(),
    })
    .await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let app = mock_app();
    let mut mapping = IdMapping::new();
    mapping.org_units.insert(1, 2);
    let mut summary = MigrationSummary::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, soap, app.handle(), &cancel);
    migrate_users(&env, &mut mapping, &mut summary)
        .await
        .expect("runs");
    (summary, dest_server)
}

#[tokio::test]
async fn user_migration_fails_visibly_when_soap_is_missing_or_disabled() {
    let (summary, dest_server) = run_users(None).await;
    assert_eq!(summary.users_failed.len(), 1);
    assert!(
        summary.users_failed[0].starts_with("pat@example.com:")
            && summary.users_failed[0].contains("SOAP"),
        "the reason is reported with the login: {:?}",
        summary.users_failed
    );
    assert!(dest_server
        .requests()
        .iter()
        .all(|request| request.method == "GET"));
    let result = build_migration_result(&summary, false);
    assert!(!result.success);
    assert!(result
        .issues
        .iter()
        .any(|issue| issue.starts_with("pat@example.com:")));

    let soap_server = soap_disabled_server().await;
    let soap = NcSoapClient::new(&soap_server.base_url, "test-jwt");
    let (summary, _) = run_users(Some(&soap)).await;
    assert_eq!(soap_server.count("POST", SOAP_PATH), 1);
    assert_eq!(summary.users_failed.len(), 1);
    assert!(
        summary.users_failed[0].starts_with("pat@example.com: SOAP userAdd failed"),
        "{:?}",
        summary.users_failed
    );
    assert!(!build_migration_result(&summary, false).success);
}
