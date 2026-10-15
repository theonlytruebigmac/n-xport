use super::{
    device_is_in_scope, export_is_partial, fetch_iterative, site_is_in_scope, DeviceAssetFlat,
    DeviceCsvRow, ExportOutput, Phase,
};
use crate::api::NcClient;
use crate::models::{Device, DeviceAsset, Site};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

#[derive(Serialize)]
struct TestRecord {
    id: i32,
}

fn count_exported_records(export_csv: bool, export_json: bool) -> usize {
    let dir = TempDir::new().expect("temporary directory");
    let records = vec![TestRecord { id: 1 }, TestRecord { id: 2 }];
    let mut out = ExportOutput::new(dir.path(), export_csv, export_json);

    out.write(&records, "records");

    assert!(out.errors.is_empty());
    out.total_records
}

#[test]
fn export_record_count_is_independent_of_selected_formats() {
    assert_eq!(count_exported_records(true, false), 2);
    assert_eq!(count_exported_records(false, true), 2);
    assert_eq!(count_exported_records(true, true), 2);
}

#[test]
fn export_issue_classification_distinguishes_partial_and_cancelled_runs() {
    let warning = vec!["one device could not be fetched".to_string()];
    let error = vec!["failed to write output".to_string()];
    assert!(export_is_partial(false, &warning, &[]));
    assert!(export_is_partial(false, &[], &error));
    assert!(!export_is_partial(false, &[], &[]));
    assert!(!export_is_partial(true, &warning, &error));
}

#[test]
fn site_scope_includes_org_unit_and_legacy_parent_field_shapes_only() {
    let customers = HashSet::from([10]);
    let sites = [
        serde_json::json!({"siteId": 101, "siteName": "Parent-linked", "parentId": 10}),
        serde_json::json!({"siteId": 102, "siteName": "Customer-linked", "customerId": 10}),
        serde_json::json!({"siteId": 103, "siteName": "Legacy-customer", "customerid": 10}),
        serde_json::json!({"siteId": 104, "siteName": "Org-unit-linked", "orgUnitId": 10}),
        serde_json::json!({"siteId": 105, "siteName": "SO-linked", "serviceOrgId": 1}),
        serde_json::json!({"siteId": 106, "siteName": "Legacy-SO-linked", "service_orgid": 1}),
        serde_json::json!({"siteId": 107, "siteName": "Foreign", "parentId": 99, "customerId": 99}),
    ]
    .into_iter()
    .map(|value| serde_json::from_value::<Site>(value).expect("site fixture"))
    .collect::<Vec<_>>();

    let included = sites
        .iter()
        .filter(|site| site_is_in_scope(site, 1, &customers))
        .map(|site| site.site_id)
        .collect::<HashSet<_>>();

    assert_eq!(included, HashSet::from([101, 102, 103, 104, 105, 106]));
}

#[test]
fn device_scope_matches_service_org_customer_and_site_ids() {
    let scope = HashSet::from([1, 10, 100]);
    let devices = [
        serde_json::json!({"deviceId": 201, "orgUnitId": 10}),
        serde_json::json!({"deviceId": 202, "customerId": 10}),
        serde_json::json!({"deviceId": 203, "siteId": 100}),
        serde_json::json!({"deviceId": 204, "soId": 1}),
        serde_json::json!({"deviceId": 205, "orgUnitId": 99, "customerId": 99, "siteId": 99, "soId": 99}),
        serde_json::json!({"deviceId": 206})
    ]
    .into_iter()
    .map(|value| serde_json::from_value::<Device>(value).expect("device fixture"))
    .collect::<Vec<_>>();

    let included = devices
        .iter()
        .filter(|device| device_is_in_scope(device, &scope))
        .map(|device| device.device_id)
        .collect::<HashSet<_>>();

    assert_eq!(included, HashSet::from([201, 202, 203, 204]));
}

#[test]
fn split_export_record_count_is_not_doubled_for_both_formats() {
    let dir = TempDir::new().expect("temporary directory");
    let csv_records = vec![TestRecord { id: 1 }, TestRecord { id: 2 }];
    let json_records = vec![TestRecord { id: 1 }, TestRecord { id: 2 }];
    let mut out = ExportOutput::new(dir.path(), true, true);

    out.write_split(&csv_records, &json_records, "records");

    assert!(out.errors.is_empty());
    assert_eq!(out.total_records, 2);
}

#[test]
fn device_status_is_flattened_for_csv_and_retained_in_json() {
    let dir = TempDir::new().expect("temporary directory");
    let device: Device = serde_json::from_value(serde_json::json!({
        "deviceId": 42,
        "deviceStatus": {"state": 1, "status": 2},
        "lastApplianceCheckinTimeLocal": "2025-02-01T05:00:00-05:00"
    }))
    .expect("device fixture should deserialize");
    let csv_rows = vec![DeviceCsvRow::from(&device)];
    let devices = vec![device];
    let mut out = ExportOutput::new(dir.path(), true, true);

    out.write_split(&csv_rows, &devices, "devices");

    assert!(out.errors.is_empty());
    assert_eq!(out.total_records, 1);
    let csv = std::fs::read_to_string(dir.path().join("devices.csv"))
        .expect("device CSV should be written");
    assert!(csv.contains("deviceStatusState"));
    assert!(csv.contains(",1,2"));
    let json: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("devices.json")).expect("device JSON should exist"),
    )
    .expect("device JSON should be valid");
    assert_eq!(json[0]["deviceStatus"]["state"], 1);
}

#[test]
fn asset_json_retains_full_categories_and_links_while_csv_is_flattened() {
    let dir = TempDir::new().expect("temporary directory");
    let mut asset: DeviceAsset = serde_json::from_value(serde_json::json!({
        "data": {
            "computersystem": {
                "manufacturer": "Example",
                "model": "Model X"
            },
            "_extra": {
                "networkadapter": {
                    "macaddress": "00:11:22:33:44:55"
                }
            }
        },
        "_links": {
            "self": "/api/devices/42/assets"
        }
    }))
    .expect("asset fixture should deserialize");
    asset.device_id = 42;
    let csv_assets = vec![DeviceAssetFlat::from(&asset)];
    let assets = vec![asset];
    let mut out = ExportOutput::new(dir.path(), true, true);

    out.write_split(&csv_assets, &assets, "device_assets");

    assert!(out.errors.is_empty());
    assert_eq!(out.total_records, 1);
    let csv = std::fs::read_to_string(dir.path().join("device_assets.csv"))
        .expect("asset CSV should exist");
    assert!(csv.contains("systemModel"));
    let json: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("device_assets.json")).expect("asset JSON should exist"),
    )
    .expect("asset JSON should be valid");
    assert_eq!(
        json[0]["data"]["_extra"]["networkadapter"]["macaddress"],
        "00:11:22:33:44:55"
    );
    assert_eq!(json[0]["_links"]["self"], "/api/devices/42/assets");
}

#[test]
fn iterative_export_does_not_start_requests_after_cancellation() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let client = NcClient::new("https://localhost");
    let cancelled = AtomicBool::new(true);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut warnings = Vec::new();
    let progress = |_: &str, _: &str, _: f32| {};
    let org_units = HashSet::from([1]);
    let request_calls = calls.clone();

    let result = runtime.block_on(fetch_iterative(
        &client,
        &org_units,
        move |_, _| {
            let calls = request_calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok::<Vec<i64>, crate::error::ApiError>(Vec::new())
            })
        },
        Phase::new("test", 0.0, 0.0),
        &progress,
        &mut warnings,
        &cancelled,
    ));

    assert!(result.is_empty());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn maps_openapi_asset_envelope_into_export_fields() {
    let mut asset: DeviceAsset = serde_json::from_value(serde_json::json!({
        "data": {
            "computersystem": {
                "manufacturer": "VMware, Inc.",
                "model": "VMware7,1",
                "totalphysicalmemory": "2147483648"
            },
            "processor": {
                "name": "Example CPU",
                "numberofcores": "4",
                "numberofcpus": "8"
            },
            "_extra": {
                "motherboard": {
                    "manufacturer": "Example BIOS vendor",
                    "serialnumber": "ABC123",
                    "biosversion": "1.2.3"
                }
            }
        },
        "_links": {}
    }))
    .expect("OpenAPI asset envelope should deserialize");
    asset.device_id = 42;

    let flat = DeviceAssetFlat::from(&asset);
    assert_eq!(flat.device_id, 42);
    assert_eq!(flat.system_manufacturer.as_deref(), Some("VMware, Inc."));
    assert_eq!(flat.system_model.as_deref(), Some("VMware7,1"));
    assert_eq!(flat.total_physical_memory, Some(2_147_483_648));
    assert_eq!(flat.processor_name.as_deref(), Some("Example CPU"));
    assert_eq!(flat.processor_cores, Some(4));
    assert_eq!(flat.processor_logical_processors, Some(8));
    assert_eq!(
        flat.bios_manufacturer.as_deref(),
        Some("Example BIOS vendor")
    );
    assert_eq!(flat.bios_serial_number.as_deref(), Some("ABC123"));
    assert_eq!(flat.bios_version.as_deref(), Some("1.2.3"));
}

/// Write real records from a live server through the exporter's own CSV and JSON
/// shapes. Ignored by default; needs NC_LIVE_BASE_URL, NC_LIVE_JWT_FILE, NC_LIVE_SO_ID.
/// Read-only: it only issues GET requests.
#[tokio::test]
#[ignore = "needs a live server"]
async fn live_records_serialize_through_the_export_shapes() {
    use crate::models::{
        AccessGroupCsvRow, DevicePropertyCsvRow, OrgPropertyCsvRow, UserCsvRow, UserRoleCsvRow,
    };
    let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {}", name));
    let jwt = std::fs::read_to_string(env("NC_LIVE_JWT_FILE")).expect("jwt file");
    let so_id: i64 = env("NC_LIVE_SO_ID").parse().expect("numeric SO id");
    let client = NcClient::new(&env("NC_LIVE_BASE_URL"));
    client.authenticate(jwt.trim()).await.expect("authenticate");

    let customers = client.get_customers_by_so(so_id).await.expect("customers");
    let mut org_units = vec![so_id];
    org_units.extend(customers.iter().map(|c| c.customer_id));

    let dir = TempDir::new().expect("temp dir");
    let mut out = ExportOutput::new(dir.path(), true, true);

    let mut users = Vec::new();
    let mut groups = Vec::new();
    let mut roles = Vec::new();
    let mut props = Vec::new();
    let mut devices = Vec::new();
    for ou in &org_units {
        users.extend(client.get_users_by_org_unit(*ou).await.expect("users"));
        groups.extend(client.get_access_groups(*ou).await.expect("groups"));
        roles.extend(client.get_user_roles(*ou).await.expect("roles"));
        props.extend(client.get_org_properties(*ou).await.expect("org props"));
        devices.extend(client.get_devices_by_org_unit(*ou).await.expect("devices"));
    }
    let mut seen = HashSet::new();
    devices.retain(|d: &Device| seen.insert(d.device_id));

    let csv_rows: Vec<UserCsvRow> = users.iter().map(Into::into).collect();
    out.write_split(&csv_rows, &users, "users");
    let csv_rows: Vec<AccessGroupCsvRow> = groups.iter().map(Into::into).collect();
    out.write_split(&csv_rows, &groups, "access_groups");
    let csv_rows: Vec<UserRoleCsvRow> = roles.iter().map(Into::into).collect();
    out.write_split(&csv_rows, &roles, "user_roles");
    let csv_rows: Vec<OrgPropertyCsvRow> = props.iter().map(Into::into).collect();
    out.write_split(&csv_rows, &props, "org_properties");
    let csv_rows: Vec<DeviceCsvRow> = devices.iter().map(Into::into).collect();
    out.write_split(&csv_rows, &devices, "devices");

    let mut device_props = Vec::new();
    let mut assets = Vec::new();
    let mut no_assets = 0;
    for device in devices.iter().take(15) {
        device_props.extend(
            client
                .get_device_properties(device.device_id)
                .await
                .expect("device props"),
        );
        // Devices without asset data return 404; the real export records that as a warning.
        match client.get_device_assets(device.device_id).await {
            Ok(mut asset) => {
                asset.device_id = device.device_id;
                assets.push(asset);
            }
            Err(crate::error::ApiError::NotFound(_)) => no_assets += 1,
            Err(e) => panic!("assets for device {}: {}", device.device_id, e),
        }
    }
    let csv_rows: Vec<DevicePropertyCsvRow> = device_props.iter().map(Into::into).collect();
    out.write_split(&csv_rows, &device_props, "device_properties");
    let csv_rows: Vec<DeviceAssetFlat> = assets.iter().map(DeviceAssetFlat::from).collect();
    out.write_split(&csv_rows, &assets, "device_assets");

    println!(
        "users {} groups {} roles {} org props {} devices {} device props {} assets {} (no asset data: {})",
        users.len(), groups.len(), roles.len(), props.len(), devices.len(), device_props.len(), assets.len(), no_assets
    );
    println!(
        "records written: {}, files: {}",
        out.total_records,
        out.files_created.len()
    );
    assert!(
        out.errors.is_empty(),
        "export shapes failed on live data: {:?}",
        out.errors
    );
}
