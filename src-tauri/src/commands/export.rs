//! Export-related Tauri commands

use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::{Emitter, State, Window};

use crate::api::client::NcClient;
use crate::commands::connection::AppState;
use crate::export::{export_to_csv, export_to_json};
use crate::models::{
    AccessGroupCsvRow, Device, DeviceAsset, DevicePropertyCsvRow, ExportOptions, OrgPropertyCsvRow,
    ProgressUpdate, UserCsvRow, UserRoleCsvRow,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceCsvRow {
    device_id: i64,
    uri: Option<String>,
    remote_control_uri: Option<String>,
    source_uri: Option<String>,
    long_name: Option<String>,
    device_class: Option<String>,
    device_class_label: Option<String>,
    description: Option<String>,
    is_probe: Option<bool>,
    os_id: Option<String>,
    supported_os: Option<String>,
    supported_os_label: Option<String>,
    discovered_name: Option<String>,
    last_logged_in_user: Option<String>,
    still_logged_in: Option<bool>,
    license_mode: Option<String>,
    org_unit_id: Option<i64>,
    so_id: Option<i64>,
    so_name: Option<String>,
    customer_id: Option<i64>,
    customer_name: Option<String>,
    site_id: Option<i64>,
    site_name: Option<String>,
    appliance_id: Option<i64>,
    last_appliance_checkin_time: Option<String>,
    last_appliance_checkin_time_local: Option<String>,
    device_status_state: Option<i32>,
    device_status_status: Option<i32>,
}

impl From<&Device> for DeviceCsvRow {
    fn from(device: &Device) -> Self {
        Self {
            device_id: device.device_id,
            uri: device.uri.clone(),
            remote_control_uri: device.remote_control_uri.clone(),
            source_uri: device.source_uri.clone(),
            long_name: device.long_name.clone(),
            device_class: device.device_class.clone(),
            device_class_label: device.device_class_label.clone(),
            description: device.description.clone(),
            is_probe: device.is_probe,
            os_id: device.os_id.clone(),
            supported_os: device.supported_os.clone(),
            supported_os_label: device.supported_os_label.clone(),
            discovered_name: device.discovered_name.clone(),
            last_logged_in_user: device.last_logged_in_user.clone(),
            still_logged_in: device.still_logged_in,
            license_mode: device.license_mode.clone(),
            org_unit_id: device.org_unit_id,
            so_id: device.so_id,
            so_name: device.so_name.clone(),
            customer_id: device.customer_id,
            customer_name: device.customer_name.clone(),
            site_id: device.site_id,
            site_name: device.site_name.clone(),
            appliance_id: device.appliance_id,
            last_appliance_checkin_time: device.last_appliance_checkin_time.clone(),
            last_appliance_checkin_time_local: device.last_appliance_checkin_time_local.clone(),
            device_status_state: device
                .device_status
                .as_ref()
                .and_then(|status| status.state),
            device_status_status: device
                .device_status
                .as_ref()
                .and_then(|status| status.status),
        }
    }
}

/// Flattened device asset for CSV-friendly export
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAssetFlat {
    pub device_id: i64,
    // ComputerSystem
    pub system_manufacturer: Option<String>,
    pub system_model: Option<String>,
    pub domain: Option<String>,
    pub domain_role: Option<String>,
    // BIOS
    pub bios_manufacturer: Option<String>,
    pub bios_name: Option<String>,
    pub bios_serial_number: Option<String>,
    pub bios_version: Option<String>,
    // Processor (first)
    pub processor_name: Option<String>,
    pub processor_manufacturer: Option<String>,
    pub processor_max_clock_speed: Option<i64>,
    pub processor_cores: Option<i32>,
    pub processor_logical_processors: Option<i32>,
    // Memory
    pub total_physical_memory: Option<i64>,
    pub available_physical_memory: Option<i64>,
    // Disk (first)
    pub disk_name: Option<String>,
    pub disk_size: Option<i64>,
    pub disk_free_space: Option<i64>,
}

fn asset_value<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    value?
        .as_object()?
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
}

fn first_asset_item(value: Option<&Value>) -> Option<&Value> {
    match asset_value(value, "list").and_then(Value::as_array) {
        Some(items) => items.first(),
        None => value,
    }
}

fn asset_string(value: Option<&Value>, key: &str) -> Option<String> {
    match asset_value(value, key)? {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn asset_i64(value: Option<&Value>, key: &str) -> Option<i64> {
    match asset_value(value, key)? {
        Value::Number(value) => value.as_i64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

impl From<&DeviceAsset> for DeviceAssetFlat {
    fn from(a: &DeviceAsset) -> Self {
        let cs = a.computer_system.as_ref();
        let bios = a.bios.as_ref();
        let proc = a.processor.as_ref().and_then(|v| v.first());
        let mem = a.memory.as_ref();
        let disk = a.disk_drive.as_ref().and_then(|v| v.first());
        let data = a.data.as_ref();
        let system_data = asset_value(data, "computersystem");
        let bios_data = asset_value(data, "bios")
            .or_else(|| asset_value(asset_value(data, "_extra"), "motherboard"));
        let processor_data = first_asset_item(asset_value(data, "processor"));
        let extra_data = asset_value(data, "_extra");
        let disk_data = first_asset_item(
            asset_value(data, "diskdrive")
                .or_else(|| asset_value(extra_data, "diskdrive"))
                .or_else(|| asset_value(extra_data, "logicaldisk")),
        );

        DeviceAssetFlat {
            device_id: a.device_id,
            system_manufacturer: cs
                .and_then(|c| c.manufacturer.clone())
                .or_else(|| asset_string(system_data, "manufacturer")),
            system_model: cs
                .and_then(|c| c.model.clone())
                .or_else(|| asset_string(system_data, "model")),
            domain: cs.and_then(|c| c.domain.clone()),
            domain_role: cs.and_then(|c| c.domain_role.clone()),
            bios_manufacturer: bios
                .and_then(|b| b.manufacturer.clone())
                .or_else(|| asset_string(bios_data, "manufacturer")),
            bios_name: bios
                .and_then(|b| b.name.clone())
                .or_else(|| asset_string(bios_data, "name")),
            bios_serial_number: bios
                .and_then(|b| b.serial_number.clone())
                .or_else(|| asset_string(bios_data, "serialnumber")),
            bios_version: bios
                .and_then(|b| b.version.clone())
                .or_else(|| asset_string(bios_data, "biosversion"))
                .or_else(|| asset_string(bios_data, "version")),
            processor_name: proc
                .and_then(|p| p.name.clone())
                .or_else(|| asset_string(processor_data, "name")),
            processor_manufacturer: proc.and_then(|p| p.manufacturer.clone()),
            processor_max_clock_speed: proc
                .and_then(|p| p.max_clock_speed)
                .or_else(|| asset_i64(processor_data, "maxclockspeed")),
            processor_cores: proc
                .and_then(|p| p.number_of_cores)
                .or_else(|| {
                    asset_i64(processor_data, "numberofcores")
                        .and_then(|n| n.try_into().ok())
                }),
            processor_logical_processors: proc
                .and_then(|p| p.number_of_logical_processors)
                .or_else(|| {
                    asset_i64(processor_data, "numberoflogicalprocessors")
                        .or_else(|| asset_i64(processor_data, "numberofcpus"))
                        .and_then(|n| n.try_into().ok())
                }),
            total_physical_memory: mem
                .and_then(|m| m.total_physical_memory)
                .or_else(|| asset_i64(system_data, "totalphysicalmemory")),
            available_physical_memory: mem.and_then(|m| m.available_physical_memory),
            disk_name: disk
                .and_then(|d| d.name.clone())
                .or_else(|| asset_string(disk_data, "name")),
            disk_size: disk
                .and_then(|d| d.size)
                .or_else(|| asset_i64(disk_data, "size")),
            disk_free_space: disk
                .and_then(|d| d.free_space)
                .or_else(|| asset_i64(disk_data, "freespace")),
        }
    }
}

/// Export result
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub success: bool,
    pub cancelled: bool,
    pub partial: bool,
    pub message: String,
    pub files_created: Vec<String>,
    /// Number of logical data records written, counting each record once even
    /// when both CSV and JSON output are enabled.
    pub total_records: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

fn export_is_partial(cancelled: bool, warnings: &[String], errors: &[String]) -> bool {
    !cancelled && (!warnings.is_empty() || !errors.is_empty())
}

/// Helper to write data in the requested formats (CSV/JSON), collecting files and errors.
fn write_export_files<T: Serialize>(
    data: &[T],
    output_path: &Path,
    name: &str,
    export_csv: bool,
    export_json: bool,
    files_created: &mut Vec<String>,
    total_records: &mut usize,
    errors: &mut Vec<String>,
) {
    if data.is_empty() {
        return;
    }

    let mut wrote_data = false;
    if export_csv {
        let path = output_path.join(format!("{}.csv", name));
        match export_to_csv(data, &path) {
            Ok(_) => {
                files_created.push(path.display().to_string());
                wrote_data = true;
            }
            Err(e) => {
                errors.push(format!("Failed to write {}.csv: {}", name, e));
            }
        }
    }
    if export_json {
        let path = output_path.join(format!("{}.json", name));
        match export_to_json(data, &path) {
            Ok(_) => {
                files_created.push(path.display().to_string());
                wrote_data = true;
            }
            Err(e) => {
                errors.push(format!("Failed to write {}.json: {}", name, e));
            }
        }
    }
    if wrote_data {
        *total_records += data.len();
    }
}

/// Like `write_export_files` but uses a separate flattened shape for CSV
/// while the richer shape is preserved for JSON.
fn write_split_export_files<C: Serialize, J: Serialize>(
    csv_data: &[C],
    json_data: &[J],
    output_path: &Path,
    name: &str,
    export_csv: bool,
    export_json: bool,
    files_created: &mut Vec<String>,
    total_records: &mut usize,
    errors: &mut Vec<String>,
) {
    if json_data.is_empty() {
        return;
    }

    let mut wrote_data = false;
    if export_csv {
        let path = output_path.join(format!("{}.csv", name));
        match export_to_csv(csv_data, &path) {
            Ok(_) => {
                files_created.push(path.display().to_string());
                wrote_data = true;
            }
            Err(e) => {
                errors.push(format!("Failed to write {}.csv: {}", name, e));
            }
        }
    }
    if export_json {
        let path = output_path.join(format!("{}.json", name));
        match export_to_json(json_data, &path) {
            Ok(_) => {
                files_created.push(path.display().to_string());
                wrote_data = true;
            }
            Err(e) => {
                errors.push(format!("Failed to write {}.json: {}", name, e));
            }
        }
    }
    if wrote_data {
        *total_records += json_data.len();
    }
}

/// Open a directory in the default OS file explorer
#[tauri::command]
pub async fn open_directory(path: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(path);
    // Resolve absolute path to handle relative paths like "./nc_export"
    let abs_path = std::fs::canonicalize(&path)
        .map_err(|e| format!("Directory does not exist yet (run export first): {}", e))?;

    tracing::info!("Opening directory: {:?}", abs_path);
    open::that(abs_path).map_err(|e| format!("Failed to open directory: {}", e))?;
    Ok(())
}

/// Start data export
#[tauri::command]
pub async fn start_export(
    window: Window,
    output_dir: String,
    options: ExportOptions,
    formats: Vec<String>,
    service_org_id: i64,
    state: State<'_, AppState>,
) -> std::result::Result<ExportResult, String> {
    let client = state.client.lock().await;

    let client = match &*client {
        Some(c) => c,
        None => return Err("Not connected".to_string()),
    };

    let output_path = PathBuf::from(&output_dir);
    let export_csv = formats.iter().any(|f| f == "csv");
    let export_json = formats.iter().any(|f| f == "json");

    let mut files_created = Vec::new();
    let mut total_records = 0;
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Reset cancellation token
    state.cancel_token.store(false, Ordering::Relaxed);
    let cancel_token = state.cancel_token.clone();

    // Scan hierarchy if we need deep items
    let needs_hierarchy = options.sites
        || options.users
        || options.devices
        || options.access_groups
        || options.user_roles
        || options.org_properties
        || options.device_properties
        || options.device_assets;

    let mut valid_ou_ids: HashSet<i64> = HashSet::new();
    // Also track specific sets for better filtering logic
    let mut customer_ids: HashSet<i64> = HashSet::new();
    // Store fetched data to avoid re-fetching
    let mut fetched_service_orgs = Vec::new();
    let mut fetched_customers = Vec::new();
    let mut fetched_sites = Vec::new();

    // Helper to emit progress
    let emit_progress = |phase: &str, message: &str, percent: f32| {
        let _ = window.emit(
            "export-progress",
            ProgressUpdate {
                phase: phase.to_string(),
                message: message.to_string(),
                percent,
                current: 0,
                total: 0,
            },
        );
    };

    emit_progress("Discovery", "Starting export...", 0.0);

    // 1. Always start with the Target Service Org
    valid_ou_ids.insert(service_org_id);
    match client.get_service_org_by_id(service_org_id).await {
        Ok(so) => fetched_service_orgs.push(so),
        Err(e) => {
            let msg = format!("Failed to fetch target Service Org: {}", e);
            tracing::error!("{}", msg);
            errors.push(msg);
        }
    }

    // 2. Scan Hierarchy (Customers & Sites)
    if !cancel_token.load(Ordering::Relaxed) && (needs_hierarchy || options.customers) {
        emit_progress("Discovery", "Scanning Customers...", 5.0);
        match client.get_customers_by_so(service_org_id).await {
            Ok(customers) => {
                for c in &customers {
                    valid_ou_ids.insert(c.customer_id);
                    customer_ids.insert(c.customer_id);
                }
                fetched_customers = customers;
            }
            Err(e) => {
                let msg = format!("Failed to fetch customers: {}", e);
                tracing::error!("{}", msg);
                errors.push(msg);
            }
        }
    }

    if !cancel_token.load(Ordering::Relaxed) && (needs_hierarchy || options.sites) {
        emit_progress("Discovery", "Scanning Sites...", 10.0);
        // Fetch ALL sites and filter (API limitation)
        match client.get_sites_by_so(service_org_id).await {
            Ok(mut sites) => {
                // Filter sites that belong to finding hierarchy
                sites.retain(|site| site_is_in_scope(site, service_org_id, &customer_ids));

                for s in &sites {
                    if let Some(oid) = s.org_unit_id {
                        valid_ou_ids.insert(oid);
                    }
                    valid_ou_ids.insert(s.site_id);
                }
                fetched_sites = sites;
            }
            Err(e) => {
                let msg = format!("Failed to fetch sites: {}", e);
                tracing::error!("{}", msg);
                errors.push(msg);
            }
        }
    }

    tracing::info!(
        "Hierarchy scan complete. Found {} valid Org Units.",
        valid_ou_ids.len()
    );

    // --- EXECUTE EXPORTS ---

    // Service Orgs
    if !cancel_token.load(Ordering::Relaxed) && options.service_orgs {
        write_export_files(
            &fetched_service_orgs, &output_path, "service_orgs",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Customers
    if !cancel_token.load(Ordering::Relaxed) && options.customers {
        write_export_files(
            &fetched_customers, &output_path, "customers",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Sites
    if !cancel_token.load(Ordering::Relaxed) && options.sites {
        write_export_files(
            &fetched_sites, &output_path, "sites",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Users (ITERATIVE FETCH)
    if !cancel_token.load(Ordering::Relaxed) && options.users {
        emit_progress("Users", "Iterating Org Units...", 20.0);
        let all_users = fetch_iterative(
            client, &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_users_by_org_unit(ou_id)),
            "Users", &emit_progress, 20.0, 5.0, &mut warnings, &cancel_token,
        ).await;

        // Deduplicate users by user_id
        let mut seen_ids: HashSet<i64> = HashSet::new();
        let unique_users: Vec<_> = all_users.into_iter().filter(|u: &crate::models::User| {
            seen_ids.insert(u.user_id)
        }).collect();

        tracing::info!("Fetched {} unique users ({} before dedup).", unique_users.len(), seen_ids.len());

        let csv_rows: Vec<UserCsvRow> = unique_users.iter().map(Into::into).collect();
        write_split_export_files(
            &csv_rows, &unique_users, &output_path, "users",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Devices (GLOBAL FETCH + FILTER)
    // Cache the filtered device list so device_properties/device_assets can reuse it
    let mut cached_device_list: Option<Vec<crate::models::Device>> = None;

    if !cancel_token.load(Ordering::Relaxed) && options.devices {
        emit_progress("Devices", "Fetching system-wide devices...", 40.0);
        match get_scoped_devices(client, &valid_ou_ids, &mut None).await {
            Ok(filtered_devices) => {
                let csv_rows: Vec<DeviceCsvRow> =
                    filtered_devices.iter().map(Into::into).collect();
                write_split_export_files(
                    &csv_rows, &filtered_devices, &output_path, "devices",
                    export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
                );

                // Cache for device_properties / device_assets reuse
                if options.device_properties || options.device_assets {
                    cached_device_list = Some(filtered_devices);
                }
            }
            Err(msg) => {
                tracing::error!("{}", msg);
                errors.push(msg);
            }
        }
    }

    // --- ITERATIVE EXPORTS ---
    // For Access Groups, User Roles, Org Properties, we iterate valid OUs

    // Access Groups
    if !cancel_token.load(Ordering::Relaxed) && options.access_groups {
        let all_data = fetch_iterative(
            client, &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_access_groups(ou_id)),
            "Access Groups", &emit_progress, 60.0, 5.0, &mut warnings, &cancel_token,
        ).await;

        let csv_rows: Vec<AccessGroupCsvRow> = all_data.iter().map(Into::into).collect();
        write_split_export_files(
            &csv_rows, &all_data, &output_path, "access_groups",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // User Roles
    if !cancel_token.load(Ordering::Relaxed) && options.user_roles {
        let all_data = fetch_iterative(
            client, &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_user_roles(ou_id)),
            "User Roles", &emit_progress, 70.0, 5.0, &mut warnings, &cancel_token,
        ).await;

        let csv_rows: Vec<UserRoleCsvRow> = all_data.iter().map(Into::into).collect();
        write_split_export_files(
            &csv_rows, &all_data, &output_path, "user_roles",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Org Properties
    if !cancel_token.load(Ordering::Relaxed) && options.org_properties {
        let all_data = fetch_iterative(
            client, &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_org_properties(ou_id)),
            "Org Properties", &emit_progress, 80.0, 5.0, &mut warnings, &cancel_token,
        ).await;

        let csv_rows: Vec<OrgPropertyCsvRow> = all_data.iter().map(Into::into).collect();
        write_split_export_files(
            &csv_rows, &all_data, &output_path, "org_properties",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Device Properties (iterate filtered devices with bounded concurrency)
    if !cancel_token.load(Ordering::Relaxed) && options.device_properties {
        emit_progress("Device Properties", "Fetching devices for property scan...", 85.0);

        let devices_in_scope = match get_scoped_devices(client, &valid_ou_ids, &mut cached_device_list).await {
            Ok(d) => d,
            Err(msg) => {
                tracing::error!("{}", msg);
                errors.push(msg);
                Vec::new()
            }
        };

        if !devices_in_scope.is_empty() {
            use futures::stream::{self, StreamExt};
            let total_devices = devices_in_scope.len();

            let device_ids: Vec<(usize, i64)> = devices_in_scope
                .iter()
                .enumerate()
                .map(|(idx, d)| (idx, d.device_id))
                .collect();

            let results: Vec<_> = stream::iter(device_ids)
                .map(|(idx, device_id)| {
                    let client = client.clone();
                    let cancel_token = cancel_token.clone();
                    async move {
                        if cancel_token.load(Ordering::Relaxed) {
                            return None;
                        }
                        if idx % 10 == 0 {
                            tracing::info!("Device Properties: fetching {}/{}", idx, total_devices);
                        }
                        Some((device_id, client.get_device_properties(device_id).await))
                    }
                })
                .buffer_unordered(5)
                .filter_map(|result| async move { result })
                .collect()
                .await;

            let mut all_props = Vec::new();
            for (device_id, result) in results {
                match result {
                    Ok(props) => all_props.extend(props),
                    Err(e) => warnings.push(format!(
                        "Failed to fetch properties for device {}: {}", device_id, e
                    )),
                }
            }

            tracing::info!(
                "Fetched {} device properties from {} devices.",
                all_props.len(),
                total_devices
            );

            let csv_rows: Vec<DevicePropertyCsvRow> =
                all_props.iter().map(Into::into).collect();
            write_split_export_files(
                &csv_rows, &all_props, &output_path, "device_properties",
                export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
            );

            // Re-cache for device_assets if needed
            if !cancel_token.load(Ordering::Relaxed) && options.device_assets {
                cached_device_list = Some(devices_in_scope);
            }
        }
    }

    // Device Assets (iterate filtered devices with bounded concurrency)
    if !cancel_token.load(Ordering::Relaxed) && options.device_assets {
        emit_progress("Device Assets", "Fetching devices for asset scan...", 92.0);

        let devices_in_scope = match get_scoped_devices(client, &valid_ou_ids, &mut cached_device_list).await {
            Ok(d) => d,
            Err(msg) => {
                tracing::error!("{}", msg);
                errors.push(msg);
                Vec::new()
            }
        };

        if !devices_in_scope.is_empty() {
            use futures::stream::{self, StreamExt};
            let total_devices = devices_in_scope.len();

            let device_ids: Vec<(usize, i64)> = devices_in_scope
                .iter()
                .enumerate()
                .map(|(idx, d)| (idx, d.device_id))
                .collect();

            let results: Vec<_> = stream::iter(device_ids)
                .map(|(idx, device_id)| {
                    let client = client.clone();
                    let cancel_token = cancel_token.clone();
                    async move {
                        if cancel_token.load(Ordering::Relaxed) {
                            return None;
                        }
                        if idx % 10 == 0 {
                            tracing::info!("Device Assets: fetching {}/{}", idx, total_devices);
                        }
                        Some((device_id, client.get_device_assets(device_id).await))
                    }
                })
                .buffer_unordered(5)
                .filter_map(|result| async move { result })
                .collect()
                .await;

            let mut all_assets = Vec::new();
            for (device_id, result) in results {
                match result {
                    Ok(asset) => all_assets.push(asset),
                    Err(e) => warnings.push(format!(
                        "Failed to fetch assets for device {}: {}", device_id, e
                    )),
                }
            }

            tracing::info!(
                "Fetched {} device assets from {} devices.",
                all_assets.len(),
                total_devices
            );

            let csv_assets: Vec<DeviceAssetFlat> =
                all_assets.iter().map(DeviceAssetFlat::from).collect();
            write_split_export_files(
                &csv_assets, &all_assets, &output_path, "device_assets",
                export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
            );
        }
    }

    let cancelled = cancel_token.load(Ordering::Relaxed);
    let partial = export_is_partial(cancelled, &warnings, &errors);
    if cancelled {
        emit_progress("Cancelled", "Export cancelled; completed output files were kept", 100.0);
    } else {
        emit_progress("Complete", "Export finished", 100.0);
    }

    let has_errors = !errors.is_empty();
    let message = if cancelled {
        format!(
            "Export cancelled after writing {} records to {} files",
            total_records,
            files_created.len()
        )
    } else if has_errors {
        format!(
            "{} export: wrote {} records to {} files with {} warning(s) and {} error(s)",
            if files_created.is_empty() { "Failed" } else { "Partial" },
            total_records,
            files_created.len(),
            warnings.len(),
            errors.len()
        )
    } else if !warnings.is_empty() {
        format!(
            "Partial export: wrote {} records to {} files with {} warning(s)",
            total_records,
            files_created.len(),
            warnings.len()
        )
    } else {
        format!(
            "Exported {} records to {} files",
            total_records,
            files_created.len()
        )
    };

    Ok(ExportResult {
        success: !cancelled && (!has_errors || !files_created.is_empty()),
        cancelled,
        partial,
        message,
        files_created,
        total_records,
        warnings,
        errors,
    })
}

/// Get filtered devices in scope, reusing cached list if available.
/// On cache hit, the cache is consumed (taken). On cache miss, fetches fresh and filters.
async fn get_scoped_devices(
    client: &NcClient,
    valid_ou_ids: &HashSet<i64>,
    cache: &mut Option<Vec<crate::models::Device>>,
) -> Result<Vec<crate::models::Device>, String> {
    if let Some(cached) = cache.take() {
        tracing::info!("Reusing cached device list ({} devices).", cached.len());
        return Ok(cached);
    }

    match client.get_devices().await {
        Ok(all_devices) => {
            let initial = all_devices.len();
            let filtered: Vec<_> = all_devices
                .into_iter()
                .filter(|device| device_is_in_scope(device, valid_ou_ids))
                .collect();
            tracing::info!("Devices filter: {} -> {}", initial, filtered.len());
            Ok(filtered)
        }
        Err(e) => Err(format!("Failed to fetch devices: {}", e)),
    }
}

fn site_is_in_scope(
    site: &crate::models::Site,
    service_org_id: i64,
    customer_ids: &HashSet<i64>,
) -> bool {
    let matches_org_unit = |id: i64| id == service_org_id || customer_ids.contains(&id);
    site.parent_id.is_some_and(matches_org_unit)
        || site.customer_id.is_some_and(matches_org_unit)
        || site.customerid.is_some_and(matches_org_unit)
        || site.org_unit_id.is_some_and(matches_org_unit)
        || site.service_org_id == Some(service_org_id)
        || site.service_orgid == Some(service_org_id)
}

fn device_is_in_scope(device: &Device, valid_ou_ids: &HashSet<i64>) -> bool {
    device.org_unit_id.is_some_and(|id| valid_ou_ids.contains(&id))
        || device.customer_id.is_some_and(|id| valid_ou_ids.contains(&id))
        || device.site_id.is_some_and(|id| valid_ou_ids.contains(&id))
        || device.so_id.is_some_and(|id| valid_ou_ids.contains(&id))
}

/// Helper to iteratively fetch data from all org units, collecting warnings for failures.
async fn fetch_iterative<T, F>(
    client: &NcClient,
    valid_ou_ids: &HashSet<i64>,
    fetch_fn: F,
    phase_name: &str,
    emit_progress: &(dyn Fn(&str, &str, f32) + Send + Sync),
    base_percent: f32,
    percent_range: f32,
    warnings: &mut Vec<String>,
    cancel_token: &std::sync::atomic::AtomicBool,
) -> Vec<T>
where
    F: Fn(&NcClient, i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::error::ApiResult<Vec<T>>> + Send + '_>>,
    T: Send,
{
    emit_progress(phase_name, "Iterating Org Units...", base_percent);
    let mut all_data = Vec::new();
    let mut ou_list: Vec<i64> = valid_ou_ids.iter().cloned().collect();
    ou_list.sort();

    for (idx, ou_id) in ou_list.iter().enumerate() {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        if idx % 10 == 0 {
            emit_progress(
                phase_name,
                &format!("Fetching {}/{}", idx, ou_list.len()),
                base_percent + (idx as f32 / ou_list.len() as f32) * percent_range,
            );
        }
        match fetch_fn(client, *ou_id).await {
            Ok(items) => all_data.extend(items),
            Err(e) => {
                warnings.push(format!(
                    "Failed to fetch {} for OU {}: {}",
                    phase_name, ou_id, e
                ));
            }
        }
    }

    all_data
}

/// Get list of available export types
#[tauri::command]
pub fn get_export_types() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"id": "service_orgs", "name": "Service Organizations", "default": true}),
        serde_json::json!({"id": "customers", "name": "Customers", "default": true}),
        serde_json::json!({"id": "sites", "name": "Sites", "default": true}),
        serde_json::json!({"id": "devices", "name": "Devices", "default": true, "migrationComingSoon": true}),
        serde_json::json!({"id": "access_groups", "name": "Access Groups", "default": true}),
        serde_json::json!({"id": "user_roles", "name": "User Roles", "default": true}),
        serde_json::json!({"id": "org_properties", "name": "Organization Properties", "default": true, "migrationComingSoon": true}),
        serde_json::json!({"id": "users", "name": "Users", "default": true}),
        serde_json::json!({"id": "device_properties", "name": "Device Properties", "default": false, "migrationComingSoon": true}),
        serde_json::json!({"id": "device_assets", "name": "Device Assets (Hardware)", "default": false, "migrationComingSoon": true}),
    ]
}

/// Cancel a running export or migration
#[tauri::command]
pub async fn cancel_export(state: State<'_, AppState>) -> std::result::Result<(), String> {
    state.cancel_token.store(true, std::sync::atomic::Ordering::Relaxed);
    tracing::info!("Operation cancellation requested");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        device_is_in_scope, export_is_partial, fetch_iterative, site_is_in_scope,
        write_export_files, write_split_export_files, DeviceAssetFlat, DeviceCsvRow,
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
        let mut files_created = Vec::new();
        let mut total_records = 0;
        let mut errors = Vec::new();

        write_export_files(
            &records,
            dir.path(),
            "records",
            export_csv,
            export_json,
            &mut files_created,
            &mut total_records,
            &mut errors,
        );

        assert!(errors.is_empty());
        total_records
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
            serde_json::json!({"siteId": 107, "siteName": "Foreign", "parentId": 99, "customerId": 99})
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
        let mut files_created = Vec::new();
        let mut total_records = 0;
        let mut errors = Vec::new();

        write_split_export_files(
            &csv_records,
            &json_records,
            dir.path(),
            "records",
            true,
            true,
            &mut files_created,
            &mut total_records,
            &mut errors,
        );

        assert!(errors.is_empty());
        assert_eq!(total_records, 2);
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
        let mut files_created = Vec::new();
        let mut total_records = 0;
        let mut errors = Vec::new();

        write_split_export_files(
            &csv_rows,
            &devices,
            dir.path(),
            "devices",
            true,
            true,
            &mut files_created,
            &mut total_records,
            &mut errors,
        );

        assert!(errors.is_empty());
        assert_eq!(total_records, 1);
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
        let mut files_created = Vec::new();
        let mut total_records = 0;
        let mut errors = Vec::new();

        write_split_export_files(
            &csv_assets,
            &assets,
            dir.path(),
            "device_assets",
            true,
            true,
            &mut files_created,
            &mut total_records,
            &mut errors,
        );

        assert!(errors.is_empty());
        assert_eq!(total_records, 1);
        let csv = std::fs::read_to_string(dir.path().join("device_assets.csv"))
            .expect("asset CSV should exist");
        assert!(csv.contains("systemModel"));
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(dir.path().join("device_assets.json"))
                .expect("asset JSON should exist"),
        )
        .expect("asset JSON should be valid");
        assert_eq!(json[0]["data"]["_extra"]["networkadapter"]["macaddress"], "00:11:22:33:44:55");
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
            "test",
            &progress,
            0.0,
            0.0,
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
}
