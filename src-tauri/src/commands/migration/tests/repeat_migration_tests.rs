//! Repeated migrations against a stateful fake destination.

use crate::api::client::NcClient;
use crate::commands::migration::customers::migrate_customers_and_sites;
use crate::commands::migration::env::test_env;
use crate::commands::migration::summary::{build_migration_result, MigrationSummary};
use crate::commands::migration::types::IdMapping;
use crate::test_support::{authenticated_client, json_response, not_found, start, MockServer};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::test::mock_app;

pub(super) async fn source_server() -> MockServer {
    start(|method, path, _| match (method, path) {
        ("GET", "/api/service-orgs/1/customers") => json_response(json!({
            "data": [{ "customerId": 10, "customerName": "Acme" }]
        })),
        // The second site differs only by case and must not become a second dest site.
        ("GET", "/api/sites") => json_response(json!({
            "data": [
                { "siteId": 11, "siteName": "HQ", "parentId": 10 },
                { "siteId": 12, "siteName": "hq", "parentId": 10 },
                { "siteId": 13, "siteName": "Branch", "parentId": 10 },
                // `/api/sites` is server-wide: this one belongs to another service org's
                // customer and must be ignored, not migrated or reported as a failure.
                { "siteId": 99, "siteName": "Other SO site", "parentId": 999 }
            ]
        })),
        _ => not_found(),
    })
    .await
}

/// Destination SO that keeps the customers and sites it is sent.
pub(super) async fn dest_server() -> MockServer {
    let mut customers: Vec<(i64, String)> = Vec::new();
    let mut sites: Vec<(i64, i64, String)> = Vec::new();
    start(move |method, path, body| {
        let payload: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        match (method, path) {
            ("GET", "/api/service-orgs/2/customers") => json_response(json!({
                "data": customers
                    .iter()
                    .map(|(id, name)| json!({ "customerId": id, "customerName": name }))
                    .collect::<Vec<_>>()
            })),
            ("GET", "/api/sites") => json_response(json!({
                "data": sites
                    .iter()
                    .map(|(id, parent, name)| json!({
                        "siteId": id, "siteName": name, "parentId": parent
                    }))
                    .collect::<Vec<_>>()
            })),
            ("POST", "/api/service-orgs/2/customers") => {
                let id = 200 + customers.len() as i64;
                customers.push((id, payload["customerName"].as_str().unwrap().to_string()));
                json_response(json!({ "customerId": id }))
            }
            ("POST", site_path) if site_path.starts_with("/api/customers/") => {
                let parent: i64 = site_path
                    .trim_start_matches("/api/customers/")
                    .trim_end_matches("/sites")
                    .parse()
                    .expect("parent customer id");
                let id = 300 + sites.len() as i64;
                sites.push((
                    id,
                    parent,
                    payload["siteName"].as_str().unwrap().to_string(),
                ));
                json_response(json!({ "siteId": id }))
            }
            _ => not_found(),
        }
    })
    .await
}

async fn run_customers_and_sites(
    source: &NcClient,
    dest: &NcClient,
) -> (IdMapping, MigrationSummary) {
    let app = mock_app();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut mapping = IdMapping::new();
    let mut summary = MigrationSummary::default();
    let env = test_env(source, dest, None, app.handle(), &cancel);
    migrate_customers_and_sites(&env, &mut mapping, &mut summary)
        .await
        .expect("customers and sites migrate");
    (mapping, summary)
}

#[tokio::test]
async fn repeating_a_customer_and_site_migration_creates_nothing_new() {
    let source_server = source_server().await;
    let dest_server = dest_server().await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;

    let (first_mapping, first_summary) = run_customers_and_sites(&source, &dest).await;
    assert!(
        first_summary.issues().is_empty(),
        "{:?}",
        first_summary.issues()
    );
    assert_eq!(
        dest_server.count("POST", "/api/service-orgs/2/customers"),
        1
    );
    assert_eq!(
        dest_server.count("POST", "/api/customers/200/sites"),
        2,
        "HQ and its case-variant duplicate collapse into one site, plus Branch"
    );
    assert_eq!(
        first_mapping.sites[&11], first_mapping.sites[&12],
        "duplicate source sites map to the same destination site"
    );

    let (second_mapping, second_summary) = run_customers_and_sites(&source, &dest).await;
    assert!(
        second_summary.issues().is_empty(),
        "{:?}",
        second_summary.issues()
    );
    assert_eq!(
        dest_server.count("POST", "/api/service-orgs/2/customers"),
        1,
        "a repeated migration must not create customers"
    );
    assert_eq!(
        dest_server.count("POST", "/api/customers/200/sites"),
        2,
        "a repeated migration must not create sites"
    );
    assert_eq!(second_mapping.customers, first_mapping.customers);
    assert_eq!(second_mapping.sites, first_mapping.sites);
    assert!(
        second_summary.skipped.len() >= 4,
        "existing customer and sites are reported as skipped: {:?}",
        second_summary.skipped
    );

    let result = build_migration_result(&second_summary, false);
    assert!(result.success, "an all-skipped repeat run is not a failure");
    assert!(result.issues.is_empty());
    assert!(!result.skipped.is_empty());
}
