//! Source-to-destination org-unit mapping and the dependency that triggers it.

use crate::commands::migration::customers::map_existing_org_units;
use crate::commands::migration::env::test_env;
use crate::commands::migration::summary::MigrationSummary;
use crate::commands::migration::types::{IdMapping, MigrationOptions};
use crate::test_support::{authenticated_client, json_response, not_found, start};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::test::mock_app;

fn options(
    customers: bool,
    roles: bool,
    users: bool,
    groups: bool,
    props: bool,
    devices: bool,
) -> MigrationOptions {
    MigrationOptions {
        customers,
        user_roles: roles,
        users,
        access_groups: groups,
        org_properties: props,
        device_properties: devices,
    }
}

#[test]
fn existing_org_units_are_mapped_whenever_a_dependent_resource_runs_without_customers() {
    assert!(!options(false, false, false, false, false, false).needs_existing_org_unit_mapping());
    assert!(!options(true, true, true, true, true, true).needs_existing_org_unit_mapping());
    assert!(options(false, true, false, false, false, false).needs_existing_org_unit_mapping());
    assert!(options(false, false, true, false, false, false).needs_existing_org_unit_mapping());
    assert!(options(false, false, false, true, false, false).needs_existing_org_unit_mapping());
    assert!(options(false, false, false, false, true, false).needs_existing_org_unit_mapping());
    assert!(options(false, false, false, false, false, true).needs_existing_org_unit_mapping());
}

#[tokio::test]
async fn existing_org_units_map_by_unique_name_and_report_missing_or_ambiguous_ones() {
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/service-orgs/1/customers") => json_response(json!({
            "data": [
                { "customerId": 10, "customerName": "Acme" },
                { "customerId": 11, "customerName": "Beta" },
                { "customerId": 12, "customerName": "Twin" }
            ]
        })),
        ("GET", "/api/customers/10/sites") => json_response(json!({
            "data": [
                { "siteId": 21, "siteName": "HQ", "parentId": 10 },
                { "siteId": 22, "siteName": "Branch", "parentId": 10 },
                { "siteId": 23, "siteName": "Annex", "parentId": 10 }
            ]
        })),
        _ => not_found(),
    })
    .await;
    let dest_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/service-orgs/2/customers") => json_response(json!({
            "data": [
                { "customerId": 200, "customerName": "ACME" },
                { "customerId": 201, "customerName": "Twin" },
                { "customerId": 202, "customerName": "twin" }
            ]
        })),
        ("GET", "/api/customers/200/sites") => json_response(json!({
            "data": [
                { "siteId": 300, "siteName": "hq", "parentId": 200 },
                { "siteId": 301, "siteName": "Annex", "parentId": 200 },
                { "siteId": 302, "siteName": "annex", "parentId": 200 }
            ]
        })),
        _ => not_found(),
    })
    .await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let app = mock_app();
    let mut mapping = IdMapping::new();
    let mut summary = MigrationSummary::default();

    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, None, app.handle(), &cancel);
    map_existing_org_units(&env, &mut mapping, &mut summary)
        .await
        .expect("mapping succeeds");

    assert_eq!(mapping.customers, HashMap::from([(10, 200)]));
    assert_eq!(mapping.sites, HashMap::from([(21, 300)]));
    assert_eq!(
        mapping.org_units,
        HashMap::from([(1, 2), (10, 200), (21, 300)]),
        "only uniquely matched org units are mapped"
    );
    let follow_up = summary.additional_manual_follow_up.join("\n");
    assert!(
        follow_up.contains("Customer 'Beta' is not present"),
        "{}",
        follow_up
    );
    assert!(
        follow_up.contains("Customer 'Twin' is ambiguous"),
        "{}",
        follow_up
    );
    assert!(
        follow_up.contains("Site 'Branch'") && follow_up.contains("not present"),
        "{}",
        follow_up
    );
    assert!(
        follow_up.contains("Site 'Annex'") && follow_up.contains("ambiguous"),
        "{}",
        follow_up
    );
    assert!(summary.issues().is_empty());
    assert!(
        dest_server
            .requests()
            .iter()
            .all(|request| request.method == "GET"),
        "mapping existing org units must never write to the destination"
    );
}
