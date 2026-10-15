//! Device properties can only move to devices that are already on the destination.

use crate::commands::migration::env::test_env;
use crate::commands::migration::properties::migrate_device_properties;
use crate::commands::migration::summary::{build_migration_result, MigrationSummary};
use crate::commands::migration::types::IdMapping;
use crate::test_support::{authenticated_client, json_response, not_found, start};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::test::mock_app;

#[tokio::test]
async fn devices_missing_from_the_destination_are_reported_once_with_what_to_do() {
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/1/devices") => json_response(json!({ "data": [
            { "deviceId": 7, "longName": "SRV-01", "orgUnitId": 1 },
            { "deviceId": 8, "longName": "SRV-02", "orgUnitId": 1 }
        ]})),
        _ => not_found(),
    })
    .await;
    let dest_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/2/devices") => json_response(json!({ "data": [] })),
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
    let env = test_env(&source, &dest, None, app.handle(), &cancel);

    migrate_device_properties(&env, &mapping, &mut summary)
        .await
        .expect("runs");

    let follow_up = summary.manual_follow_up();
    let text = follow_up.join("\n");
    assert!(
        text.contains("'SRV-01' was not found in destination OU 2"),
        "{}",
        text
    );
    assert!(
        text.contains("'SRV-02' was not found in destination OU 2"),
        "{}",
        text
    );
    let summaries: Vec<_> = follow_up
        .iter()
        .filter(|item| item.contains("not on the destination yet"))
        .collect();
    assert_eq!(
        summaries.len(),
        1,
        "one summary, not one per device: {}",
        text
    );
    assert!(summaries[0].starts_with("2 device(s)"), "{}", summaries[0]);
    assert!(
        summaries[0].contains("install or register their agents"),
        "{}",
        summaries[0]
    );
    assert!(!build_migration_result(&summary, false).success);
}
