use crate::commands::migration::env::items_until_cancelled;
use crate::commands::migration::mapping::{
    destination_device_property_id, destination_property_id,
};
use crate::commands::migration::summary::{build_migration_result, MigrationSummary};
use crate::models::{DeviceProperty, OrgProperty};
use std::sync::atomic::{AtomicBool, Ordering};

fn property(property_id: i64, label: &str) -> OrgProperty {
    OrgProperty {
        property_id,
        label: Some(label.to_string()),
        value: None,
        default_value: None,
        property_type: None,
        org_unit_id: Some(10),
        enumerated_value_list: None,
        extra: None,
    }
}

fn device_property(property_id: i64, label: &str) -> DeviceProperty {
    DeviceProperty {
        property_id,
        device_id: Some(1),
        device_name: Some("WS01".to_string()),
        label: Some(label.to_string()),
        value: None,
        default_value: None,
        property_type: None,
        enumerated_value_list: None,
        extra: None,
    }
}

#[test]
fn migration_succeeds_only_when_there_are_no_failures_or_follow_ups() {
    let result = build_migration_result(&MigrationSummary::default(), false);
    assert!(result.success);
    assert!(!result.cancelled);
    assert!(result.issues.is_empty());
    assert!(result.manual_follow_up.is_empty());
}

#[test]
fn migration_item_iteration_stops_before_the_next_item_after_cancellation() {
    let cancelled = AtomicBool::new(false);
    let mut processed = Vec::new();

    for item in items_until_cancelled(vec!["first", "second"], &cancelled) {
        processed.push(item);
        cancelled.store(true, Ordering::Relaxed);
    }

    assert_eq!(processed, vec!["first"]);
}

#[test]
fn organization_property_migration_matches_destination_definition_by_name() {
    let source = property(100, "Department");
    let destination = vec![property(900, "department")];

    assert_eq!(destination_property_id(&source, &destination), Ok(900));
}

#[test]
fn organization_property_migration_rejects_missing_or_ambiguous_definitions() {
    let source = property(100, "Department");

    assert!(destination_property_id(&source, &[]).is_err());
    assert!(destination_property_id(
        &source,
        &[property(900, "Department"), property(901, "DEPARTMENT")]
    )
    .unwrap_err()
    .contains("ambiguous"));
}

#[test]
fn device_property_migration_matches_destination_definition_by_name() {
    let source = device_property(100, "Asset Tag");

    assert_eq!(
        destination_device_property_id(&source, &[device_property(900, "asset tag")]),
        Ok(900)
    );
    assert!(destination_device_property_id(&source, &[])
        .unwrap_err()
        .contains("not found"));
}

#[test]
fn partial_migration_reports_failures_and_manual_follow_ups() {
    let summary = MigrationSummary {
        sites_failed: vec!["Site A failed".to_string()],
        skipped: vec!["Customer B already exists".to_string()],
        access_groups_existed_not_updated: vec![
            "'Operators' (at Customer 1) is missing 2 migrated user(s)".to_string(),
        ],
        additional_manual_follow_up: vec!["Device group needs device ID mapping".to_string()],
        ..MigrationSummary::default()
    };

    let result = build_migration_result(&summary, false);
    assert!(!result.success);
    assert!(!result.cancelled);
    assert_eq!(result.issues, vec!["Site A failed".to_string()]);
    assert_eq!(
        result.skipped,
        vec!["Customer B already exists".to_string()]
    );
    assert_eq!(result.manual_follow_up.len(), 2);
    assert!(result.message.contains("1 failure"));
    assert!(result.message.contains("2 manual follow-up"));
}

#[test]
fn all_failures_are_reported_as_failure() {
    let summary = MigrationSummary {
        customers_failed: vec!["Customer A failed".to_string()],
        sites_failed: vec!["Site A failed".to_string()],
        roles_failed: vec!["Role A failed".to_string()],
        users_failed: vec!["user.a".to_string()],
        access_groups_failed: vec!["Group A failed".to_string()],
        properties_failed: vec!["Property A failed".to_string()],
        fetch_failures: vec!["Fetch failed: source users".to_string()],
        ..MigrationSummary::default()
    };

    let result = build_migration_result(&summary, false);
    assert!(!result.success);
    assert_eq!(result.issues.len(), 7);
    assert!(result.manual_follow_up.is_empty());
    assert!(result.message.contains("7 failure(s)"));
}

#[test]
fn cancelled_migration_is_not_reported_as_success() {
    let result = build_migration_result(&MigrationSummary::default(), true);
    assert!(!result.success);
    assert!(result.cancelled);
    assert!(result.message.contains("cancelled"));
}
