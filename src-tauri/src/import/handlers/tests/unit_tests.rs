use crate::api::NcClient;
use crate::import::handlers::access_groups::validate_import_group_type;
use crate::import::handlers::context::ImportContext;
use crate::import::handlers::properties::{unique_property_id, validate_property_value};
use std::sync::atomic::AtomicBool;

#[test]
fn lookup_loading_stops_before_requests_after_cancellation() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let client = NcClient::new("https://localhost");
    let cancel_token = AtomicBool::new(true);

    let result = runtime.block_on(ImportContext::load(&client, 1, &cancel_token));
    assert!(matches!(result, Err(message) if message == "Import cancelled"));
}

#[test]
fn only_documented_org_unit_access_group_type_is_supported_for_import() {
    assert!(validate_import_group_type("ORG_UNIT").is_ok());
    assert!(validate_import_group_type("org_unit").is_ok());
    assert!(validate_import_group_type("DEVICE")
        .expect_err("device IDs cannot be mapped")
        .contains("device ID mapping"));
    assert!(validate_import_group_type("OTHER")
        .expect_err("unknown group type must be rejected")
        .contains("expected ORG_UNIT"));
}

#[test]
fn property_import_matches_one_existing_definition_without_using_source_ids() {
    assert_eq!(
        unique_property_id(
            " Asset Tag ",
            vec![
                (7, Some("Department".to_string())),
                (22, Some("asset tag".to_string()))
            ]
        ),
        Ok(22)
    );
    assert!(
        unique_property_id("Missing", vec![(22, Some("Asset Tag".to_string()))])
            .unwrap_err()
            .contains("not found")
    );
    assert!(unique_property_id(
        "Asset Tag",
        vec![
            (22, Some("Asset Tag".to_string())),
            (23, Some("asset tag".to_string()))
        ]
    )
    .unwrap_err()
    .contains("ambiguous"));
}

#[test]
fn enumerated_property_values_are_validated_before_writes() {
    let allowed = vec!["Operations".to_string(), "Sales".to_string()];
    assert!(validate_property_value(Some("ENUMERATED"), Some(&allowed), "operations").is_ok());
    assert!(
        validate_property_value(Some("ENUMERATED"), Some(&allowed), "Unknown")
            .unwrap_err()
            .contains("not one of")
    );
    assert!(validate_property_value(Some("TEXT"), Some(&allowed), "anything").is_ok());
    assert!(validate_property_value(Some("ENUMERATED"), Some(&allowed), "").is_ok());
}
