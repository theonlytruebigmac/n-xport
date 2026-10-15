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
pub(crate) struct DeviceCsvRow {
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
            processor_cores: proc.and_then(|p| p.number_of_cores).or_else(|| {
                asset_i64(processor_data, "numberofcores").and_then(|n| n.try_into().ok())
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

/// Where an export writes, which formats it writes, and what it has produced so far.
struct ExportOutput<'a> {
    dir: &'a Path,
    csv: bool,
    json: bool,
    files_created: Vec<String>,
    total_records: usize,
    errors: Vec<String>,
}

impl<'a> ExportOutput<'a> {
    fn new(dir: &'a Path, csv: bool, json: bool) -> Self {
        Self {
            dir,
            csv,
            json,
            files_created: Vec::new(),
            total_records: 0,
            errors: Vec::new(),
        }
    }

    /// Write data in the requested formats (CSV/JSON), collecting files and errors.
    fn write<T: Serialize>(&mut self, data: &[T], name: &str) {
        self.write_split(data, data, name);
    }

    /// Like `write` but uses a separate flattened shape for CSV while the richer
    /// shape is preserved for JSON. Each logical record is counted once, however
    /// many formats it is written in.
    fn write_split<C: Serialize, J: Serialize>(
        &mut self,
        csv_data: &[C],
        json_data: &[J],
        name: &str,
    ) {
        if json_data.is_empty() {
            return;
        }

        let mut wrote_data = false;
        if self.csv {
            let path = self.dir.join(format!("{}.csv", name));
            match export_to_csv(csv_data, &path) {
                Ok(_) => {
                    self.files_created.push(path.display().to_string());
                    wrote_data = true;
                }
                Err(e) => {
                    self.errors
                        .push(format!("Failed to write {}.csv: {}", name, e));
                }
            }
        }
        if self.json {
            let path = self.dir.join(format!("{}.json", name));
            match export_to_json(json_data, &path) {
                Ok(_) => {
                    self.files_created.push(path.display().to_string());
                    wrote_data = true;
                }
                Err(e) => {
                    self.errors
                        .push(format!("Failed to write {}.json: {}", name, e));
                }
            }
        }
        if wrote_data {
            self.total_records += json_data.len();
        }
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

    let mut out = ExportOutput::new(&output_path, export_csv, export_json);
    let mut warnings = Vec::new();

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
            out.errors.push(msg);
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
                out.errors.push(msg);
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
                out.errors.push(msg);
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
        out.write(&fetched_service_orgs, "service_orgs");
    }

    // Customers
    if !cancel_token.load(Ordering::Relaxed) && options.customers {
        out.write(&fetched_customers, "customers");
    }

    // Sites
    if !cancel_token.load(Ordering::Relaxed) && options.sites {
        out.write(&fetched_sites, "sites");
    }

    // Users (ITERATIVE FETCH)
    if !cancel_token.load(Ordering::Relaxed) && options.users {
        emit_progress("Users", "Iterating Org Units...", 20.0);
        let all_users = fetch_iterative(
            client,
            &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_users_by_org_unit(ou_id)),
            Phase::new("Users", 20.0, 5.0),
            &emit_progress,
            &mut warnings,
            &cancel_token,
        )
        .await;

        // Deduplicate users by user_id
        let mut seen_ids: HashSet<i64> = HashSet::new();
        let unique_users: Vec<_> = all_users
            .into_iter()
            .filter(|u: &crate::models::User| seen_ids.insert(u.user_id))
            .collect();

        tracing::info!(
            "Fetched {} unique users ({} before dedup).",
            unique_users.len(),
            seen_ids.len()
        );

        let csv_rows: Vec<UserCsvRow> = unique_users.iter().map(Into::into).collect();
        out.write_split(&csv_rows, &unique_users, "users");
    }

    // Devices (GLOBAL FETCH + FILTER)
    // Cache the filtered device list so device_properties/device_assets can reuse it
    let mut cached_device_list: Option<Vec<crate::models::Device>> = None;

    if !cancel_token.load(Ordering::Relaxed) && options.devices {
        emit_progress("Devices", "Fetching system-wide devices...", 40.0);
        match get_scoped_devices(client, &valid_ou_ids, &mut None).await {
            Ok(filtered_devices) => {
                let csv_rows: Vec<DeviceCsvRow> = filtered_devices.iter().map(Into::into).collect();
                out.write_split(&csv_rows, &filtered_devices, "devices");

                // Cache for device_properties / device_assets reuse
                if options.device_properties || options.device_assets {
                    cached_device_list = Some(filtered_devices);
                }
            }
            Err(msg) => {
                tracing::error!("{}", msg);
                out.errors.push(msg);
            }
        }
    }

    // --- ITERATIVE EXPORTS ---
    // For Access Groups, User Roles, Org Properties, we iterate valid OUs

    // Access Groups
    if !cancel_token.load(Ordering::Relaxed) && options.access_groups {
        let all_data = fetch_iterative(
            client,
            &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_access_groups(ou_id)),
            Phase::new("Access Groups", 60.0, 5.0),
            &emit_progress,
            &mut warnings,
            &cancel_token,
        )
        .await;

        let csv_rows: Vec<AccessGroupCsvRow> = all_data.iter().map(Into::into).collect();
        out.write_split(&csv_rows, &all_data, "access_groups");
    }

    // User Roles
    if !cancel_token.load(Ordering::Relaxed) && options.user_roles {
        let all_data = fetch_iterative(
            client,
            &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_user_roles(ou_id)),
            Phase::new("User Roles", 70.0, 5.0),
            &emit_progress,
            &mut warnings,
            &cancel_token,
        )
        .await;

        let csv_rows: Vec<UserRoleCsvRow> = all_data.iter().map(Into::into).collect();
        out.write_split(&csv_rows, &all_data, "user_roles");
    }

    // Org Properties
    if !cancel_token.load(Ordering::Relaxed) && options.org_properties {
        let all_data = fetch_iterative(
            client,
            &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_org_properties(ou_id)),
            Phase::new("Org Properties", 80.0, 5.0),
            &emit_progress,
            &mut warnings,
            &cancel_token,
        )
        .await;

        let csv_rows: Vec<OrgPropertyCsvRow> = all_data.iter().map(Into::into).collect();
        out.write_split(&csv_rows, &all_data, "org_properties");
    }

    // Device Properties (iterate filtered devices with bounded concurrency)
    if !cancel_token.load(Ordering::Relaxed) && options.device_properties {
        emit_progress(
            "Device Properties",
            "Fetching devices for property scan...",
            85.0,
        );

        let devices_in_scope =
            match get_scoped_devices(client, &valid_ou_ids, &mut cached_device_list).await {
                Ok(d) => d,
                Err(msg) => {
                    tracing::error!("{}", msg);
                    out.errors.push(msg);
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
                        "Failed to fetch properties for device {}: {}",
                        device_id, e
                    )),
                }
            }

            tracing::info!(
                "Fetched {} device properties from {} devices.",
                all_props.len(),
                total_devices
            );

            let csv_rows: Vec<DevicePropertyCsvRow> = all_props.iter().map(Into::into).collect();
            out.write_split(&csv_rows, &all_props, "device_properties");

            // Re-cache for device_assets if needed
            if !cancel_token.load(Ordering::Relaxed) && options.device_assets {
                cached_device_list = Some(devices_in_scope);
            }
        }
    }

    // Device Assets (iterate filtered devices with bounded concurrency)
    if !cancel_token.load(Ordering::Relaxed) && options.device_assets {
        emit_progress("Device Assets", "Fetching devices for asset scan...", 92.0);

        let devices_in_scope =
            match get_scoped_devices(client, &valid_ou_ids, &mut cached_device_list).await {
                Ok(d) => d,
                Err(msg) => {
                    tracing::error!("{}", msg);
                    out.errors.push(msg);
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
                        "Failed to fetch assets for device {}: {}",
                        device_id, e
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
            out.write_split(&csv_assets, &all_assets, "device_assets");
        }
    }

    let cancelled = cancel_token.load(Ordering::Relaxed);
    let partial = export_is_partial(cancelled, &warnings, &out.errors);
    if cancelled {
        emit_progress(
            "Cancelled",
            "Export cancelled; completed output files were kept",
            100.0,
        );
    } else {
        emit_progress("Complete", "Export finished", 100.0);
    }

    let has_errors = !out.errors.is_empty();
    let message = if cancelled {
        format!(
            "Export cancelled after writing {} records to {} files",
            out.total_records,
            out.files_created.len()
        )
    } else if has_errors {
        format!(
            "{} export: wrote {} records to {} files with {} warning(s) and {} error(s)",
            if out.files_created.is_empty() {
                "Failed"
            } else {
                "Partial"
            },
            out.total_records,
            out.files_created.len(),
            warnings.len(),
            out.errors.len()
        )
    } else if !warnings.is_empty() {
        format!(
            "Partial export: wrote {} records to {} files with {} warning(s)",
            out.total_records,
            out.files_created.len(),
            warnings.len()
        )
    } else {
        format!(
            "Exported {} records to {} files",
            out.total_records,
            out.files_created.len()
        )
    };

    Ok(ExportResult {
        success: !cancelled && (!has_errors || !out.files_created.is_empty()),
        cancelled,
        partial,
        message,
        files_created: out.files_created,
        total_records: out.total_records,
        warnings,
        errors: out.errors,
    })
}

/// Get filtered devices in scope, reusing cached list if available.
/// On cache hit, the cache is consumed (taken). On cache miss, fetches fresh and filters.
pub(crate) async fn get_scoped_devices(
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

pub(crate) fn site_is_in_scope(
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
    device
        .org_unit_id
        .is_some_and(|id| valid_ou_ids.contains(&id))
        || device
            .customer_id
            .is_some_and(|id| valid_ou_ids.contains(&id))
        || device.site_id.is_some_and(|id| valid_ou_ids.contains(&id))
        || device.so_id.is_some_and(|id| valid_ou_ids.contains(&id))
}

/// A named export phase and the slice of the progress bar it occupies.
#[derive(Clone, Copy)]
struct Phase<'a> {
    name: &'a str,
    base_percent: f32,
    percent_range: f32,
}

impl<'a> Phase<'a> {
    fn new(name: &'a str, base_percent: f32, percent_range: f32) -> Self {
        Self {
            name,
            base_percent,
            percent_range,
        }
    }
}

/// Helper to iteratively fetch data from all org units, collecting warnings for failures.
async fn fetch_iterative<T, F>(
    client: &NcClient,
    valid_ou_ids: &HashSet<i64>,
    fetch_fn: F,
    phase: Phase<'_>,
    emit_progress: &(dyn Fn(&str, &str, f32) + Send + Sync),
    warnings: &mut Vec<String>,
    cancel_token: &std::sync::atomic::AtomicBool,
) -> Vec<T>
where
    F: Fn(
        &NcClient,
        i64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::error::ApiResult<Vec<T>>> + Send + '_>,
    >,
    T: Send,
{
    emit_progress(phase.name, "Iterating Org Units...", phase.base_percent);
    let mut all_data = Vec::new();
    let mut ou_list: Vec<i64> = valid_ou_ids.iter().cloned().collect();
    ou_list.sort();

    for (idx, ou_id) in ou_list.iter().enumerate() {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        if idx % 10 == 0 {
            emit_progress(
                phase.name,
                &format!("Fetching {}/{}", idx, ou_list.len()),
                phase.base_percent + (idx as f32 / ou_list.len() as f32) * phase.percent_range,
            );
        }
        match fetch_fn(client, *ou_id).await {
            Ok(items) => all_data.extend(items),
            Err(e) => {
                warnings.push(format!(
                    "Failed to fetch {} for OU {}: {}",
                    phase.name, ou_id, e
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
        serde_json::json!({"id": "org_properties", "name": "Organization Properties", "default": true}),
        serde_json::json!({"id": "users", "name": "Users", "default": true}),
        serde_json::json!({"id": "device_properties", "name": "Device Properties", "default": false}),
        serde_json::json!({"id": "device_assets", "name": "Device Assets (Hardware)", "default": false, "migrationComingSoon": true}),
    ]
}

/// Cancel a running export or migration
#[tauri::command]
pub async fn cancel_export(state: State<'_, AppState>) -> std::result::Result<(), String> {
    state
        .cancel_token
        .store(true, std::sync::atomic::Ordering::Relaxed);
    tracing::info!("Operation cancellation requested");
    Ok(())
}

#[cfg(test)]
mod tests;
