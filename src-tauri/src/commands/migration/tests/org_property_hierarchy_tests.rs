//! Organization property values: listed per org unit with no org-unit ID, inherited down the
//! hierarchy, and defined only where the destination already has the definition.

use crate::commands::migration::access_groups::migrate_access_groups;
use crate::commands::migration::env::test_env;
use crate::commands::migration::properties::migrate_org_properties;
use crate::commands::migration::summary::MigrationSummary;
use crate::commands::migration::types::IdMapping;
use crate::test_support::{authenticated_client, json_response, not_found, start};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::test::mock_app;

#[tokio::test]
async fn inherited_values_are_not_rewritten_but_overrides_and_missing_definitions_are_handled() {
    // Source: Tag = "x" at the service org, inherited by customer 10, overridden to "y" at
    // customer 11. "Extra" has no definition on the destination.
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/1/custom-properties") => json_response(json!({ "data": [
            { "propertyId": 5, "propertyName": "Tag", "value": "x" },
            { "propertyId": 6, "propertyName": "Extra", "value": "e" }
        ]})),
        ("GET", "/api/org-units/10/custom-properties") => json_response(json!({ "data": [
            { "propertyId": 5, "propertyName": "Tag", "value": "x" },
            { "propertyId": 6, "propertyName": "Extra", "value": "e" }
        ]})),
        ("GET", "/api/org-units/11/custom-properties") => json_response(json!({ "data": [
            { "propertyId": 5, "propertyName": "Tag", "value": "y" },
            { "propertyId": 6, "propertyName": "Extra", "value": "e" }
        ]})),
        _ => not_found(),
    })
    .await;
    // Destination: writing the service org's value is inherited by customer 200 straight
    // away, as on N-central; customer 201 keeps its own value.
    let mut so_value = String::new();
    let mut c201_value: Option<String> = None;
    let dest_server = start(move |method, path, body| {
        let property =
            |value: &str| json!({ "propertyId": 50, "propertyName": "Tag", "value": value });
        match (method, path) {
            ("GET", "/api/org-units/2/custom-properties") => {
                json_response(json!({ "data": [property(&so_value)] }))
            }
            ("GET", "/api/org-units/200/custom-properties") => {
                json_response(json!({ "data": [property(&so_value)] }))
            }
            ("GET", "/api/org-units/201/custom-properties") => json_response(
                json!({ "data": [property(c201_value.as_deref().unwrap_or(&so_value))] }),
            ),
            ("PUT", "/api/org-units/2/custom-properties/50") => {
                so_value = serde_json::from_str::<serde_json::Value>(body).unwrap()["value"]
                    .as_str()
                    .unwrap()
                    .into();
                json_response(json!({}))
            }
            ("PUT", "/api/org-units/201/custom-properties/50") => {
                c201_value = Some(
                    serde_json::from_str::<serde_json::Value>(body).unwrap()["value"]
                        .as_str()
                        .unwrap()
                        .into(),
                );
                json_response(json!({}))
            }
            _ => not_found(),
        }
    })
    .await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let app = mock_app();
    let mut mapping = IdMapping::new();
    mapping.customers.extend([(10, 200), (11, 201)]);
    let mut summary = MigrationSummary::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, None, app.handle(), &cancel);

    migrate_org_properties(&env, &mapping, &mut summary)
        .await
        .expect("runs");

    assert!(summary.issues().is_empty(), "{:?}", summary.issues());
    assert_eq!(
        dest_server.count("PUT", "/api/org-units/2/custom-properties/50"),
        1
    );
    assert_eq!(
        dest_server.count("PUT", "/api/org-units/200/custom-properties/50"),
        0,
        "a value the customer only inherits must not be written as an override"
    );
    assert_eq!(
        dest_server.count("PUT", "/api/org-units/201/custom-properties/50"),
        1,
        "a real override is written"
    );
    assert_eq!(
        summary.properties_skipped.len(),
        1,
        "the missing definition is reported once, not at every org unit: {:?}",
        summary.properties_skipped
    );
    assert!(summary.properties_skipped[0].contains("Extra"));
    assert!(
        summary.properties_skipped[0].contains("must already exist on the destination")
            && summary.properties_skipped[0].contains("cannot create them"),
        "the message says what to do: {}",
        summary.properties_skipped[0]
    );

    // Running it again finds everything already matching.
    let mut again = MigrationSummary::default();
    migrate_org_properties(&env, &mapping, &mut again)
        .await
        .expect("runs");
    assert_eq!(
        dest_server.count("PUT", "/api/org-units/2/custom-properties/50"),
        1
    );
    assert_eq!(
        dest_server.count("PUT", "/api/org-units/201/custom-properties/50"),
        1
    );
}

/// Run the access-group step against a destination that already has the group, whose
/// members (per the detail endpoint) are `dest_members`; the migration expects users 900
/// and 901 to be in it.
async fn existing_group_summary(dest_members: Vec<i64>) -> MigrationSummary {
    let source_server = start(|method, path, _| match (method, path) {
        ("GET", "/api/org-units/1/access-groups") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/10/access-groups") => json_response(json!({ "data": [
            { "groupId": 1, "orgUnitId": 10, "groupName": "Helpdesk", "groupType": "ORG_UNIT" }
        ]})),
        _ => not_found(),
    })
    .await;
    let dest_server = start(move |method, path, _| match (method, path) {
        ("GET", "/api/org-units/2/access-groups") => json_response(json!({ "data": [] })),
        ("GET", "/api/org-units/200/access-groups") => json_response(json!({ "data": [
            { "groupId": 777, "orgUnitId": 200, "groupName": "Helpdesk", "groupType": "ORG_UNIT" }
        ]})),
        ("GET", "/api/access-groups/777") => json_response(json!({ "data": {
            "groupId": 777, "orgUnitId": 200, "groupName": "Helpdesk",
            "orgUnitIds": [200], "userIds": dest_members
        }})),
        _ => not_found(),
    })
    .await;
    let source = authenticated_client(&source_server).await;
    let dest = authenticated_client(&dest_server).await;
    let app = mock_app();
    let mut mapping = IdMapping::new();
    mapping.org_units.extend([(1, 2), (10, 200)]);
    mapping.customers.insert(10, 200);
    mapping.access_group_members.insert(1, vec![900, 901]);
    let mut summary = MigrationSummary::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let env = test_env(&source, &dest, None, app.handle(), &cancel);
    migrate_access_groups(&env, &mut mapping, &mut summary)
        .await
        .expect("groups migrate");
    summary
}

#[tokio::test]
async fn an_existing_group_needs_follow_up_only_when_a_migrated_user_is_missing_from_it() {
    let complete = existing_group_summary(vec![900, 901, 5]).await;
    assert!(
        complete.access_groups_existed_not_updated.is_empty(),
        "already has every migrated user: {:?}",
        complete.access_groups_existed_not_updated
    );
    assert!(complete.manual_follow_up().is_empty());
    assert_eq!(
        complete.skipped.len(),
        1,
        "still reported as already existing"
    );

    let partial = existing_group_summary(vec![900]).await;
    assert_eq!(
        partial.access_groups_existed_not_updated,
        vec!["'Helpdesk' (at Customer 10) is missing 1 migrated user(s)".to_string()]
    );
}
