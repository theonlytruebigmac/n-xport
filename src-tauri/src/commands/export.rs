//! Export-related Tauri commands

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::{Emitter, State, Window};

use crate::api::client::NcClient;
use crate::commands::connection::AppState;
use crate::export::{export_to_csv, export_to_json};
use crate::models::{DeviceAsset, ExportOptions, ProgressUpdate};

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

impl From<DeviceAsset> for DeviceAssetFlat {
    fn from(a: DeviceAsset) -> Self {
        let cs = a.computer_system.as_ref();
        let bios = a.bios.as_ref();
        let proc = a.processor.as_ref().and_then(|v| v.first());
        let mem = a.memory.as_ref();
        let disk = a.disk_drive.as_ref().and_then(|v| v.first());

        DeviceAssetFlat {
            device_id: a.device_id,
            system_manufacturer: cs.and_then(|c| c.manufacturer.clone()),
            system_model: cs.and_then(|c| c.model.clone()),
            domain: cs.and_then(|c| c.domain.clone()),
            domain_role: cs.and_then(|c| c.domain_role.clone()),
            bios_manufacturer: bios.and_then(|b| b.manufacturer.clone()),
            bios_name: bios.and_then(|b| b.name.clone()),
            bios_serial_number: bios.and_then(|b| b.serial_number.clone()),
            bios_version: bios.and_then(|b| b.version.clone()),
            processor_name: proc.and_then(|p| p.name.clone()),
            processor_manufacturer: proc.and_then(|p| p.manufacturer.clone()),
            processor_max_clock_speed: proc.and_then(|p| p.max_clock_speed),
            processor_cores: proc.and_then(|p| p.number_of_cores),
            processor_logical_processors: proc.and_then(|p| p.number_of_logical_processors),
            total_physical_memory: mem.and_then(|m| m.total_physical_memory),
            available_physical_memory: mem.and_then(|m| m.available_physical_memory),
            disk_name: disk.and_then(|d| d.name.clone()),
            disk_size: disk.and_then(|d| d.size),
            disk_free_space: disk.and_then(|d| d.free_space),
        }
    }
}

/// Export result
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub success: bool,
    pub message: String,
    pub files_created: Vec<String>,
    pub total_records: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
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

    if export_csv {
        let path = output_path.join(format!("{}.csv", name));
        match export_to_csv(data, &path) {
            Ok(c) => {
                files_created.push(path.display().to_string());
                *total_records += c;
            }
            Err(e) => {
                errors.push(format!("Failed to write {}.csv: {}", name, e));
            }
        }
    }
    if export_json {
        let path = output_path.join(format!("{}.json", name));
        match export_to_json(data, &path) {
            Ok(c) => {
                files_created.push(path.display().to_string());
                *total_records += c;
            }
            Err(e) => {
                errors.push(format!("Failed to write {}.json: {}", name, e));
            }
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

    let mut files_created = Vec::new();
    let mut total_records = 0;
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Reset cancellation token
    state.cancel_token.store(false, Ordering::Relaxed);
    let _cancel_token = state.cancel_token.clone();

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
    if needs_hierarchy || options.customers {
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

    if needs_hierarchy || options.sites {
        emit_progress("Discovery", "Scanning Sites...", 10.0);
        // Fetch ALL sites and filter (API limitation)
        match client.get_sites_by_so(service_org_id).await {
            Ok(mut sites) => {
                // Filter sites that belong to finding hierarchy
                sites.retain(|s| {
                    let pid_match = s.parent_id.map_or(false, |pid| {
                        customer_ids.contains(&pid) || pid == service_org_id
                    });
                    let cid_match = s.customer_id.map_or(false, |cid| {
                        customer_ids.contains(&cid) || cid == service_org_id
                    });
                    let oid_match = s.org_unit_id.map_or(false, |oid| {
                        customer_ids.contains(&oid) || oid == service_org_id
                    });

                    // Specific check for SO direct child sites
                    let sid_match = s.service_org_id.map_or(false, |sid| sid == service_org_id);

                    pid_match || cid_match || oid_match || sid_match
                });

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
    if options.service_orgs {
        write_export_files(
            &fetched_service_orgs, &output_path, "service_orgs",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Customers
    if options.customers {
        write_export_files(
            &fetched_customers, &output_path, "customers",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Sites
    if options.sites {
        write_export_files(
            &fetched_sites, &output_path, "sites",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Users (ITERATIVE FETCH)
    if options.users {
        emit_progress("Users", "Iterating Org Units...", 20.0);
        let all_users = fetch_iterative(
            client, &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_users_by_org_unit(ou_id)),
            "Users", &emit_progress, 20.0, 5.0, &mut warnings,
        ).await;

        // Deduplicate users by user_id
        let mut seen_ids: HashSet<i64> = HashSet::new();
        let unique_users: Vec<_> = all_users.into_iter().filter(|u: &crate::models::User| {
            seen_ids.insert(u.user_id)
        }).collect();

        tracing::info!("Fetched {} unique users ({} before dedup).", unique_users.len(), seen_ids.len());

        write_export_files(
            &unique_users, &output_path, "users",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Devices (GLOBAL FETCH + FILTER)
    // Cache the filtered device list so device_properties/device_assets can reuse it
    let mut cached_device_list: Option<Vec<crate::models::Device>> = None;

    if options.devices {
        emit_progress("Devices", "Fetching system-wide devices...", 40.0);
        match get_scoped_devices(client, &valid_ou_ids, &mut None).await {
            Ok(filtered_devices) => {
                write_export_files(
                    &filtered_devices, &output_path, "devices",
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
    if options.access_groups {
        let all_data = fetch_iterative(
            client, &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_access_groups(ou_id)),
            "Access Groups", &emit_progress, 60.0, 5.0, &mut warnings,
        ).await;

        write_export_files(
            &all_data, &output_path, "access_groups",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // User Roles
    if options.user_roles {
        let all_data = fetch_iterative(
            client, &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_user_roles(ou_id)),
            "User Roles", &emit_progress, 70.0, 5.0, &mut warnings,
        ).await;

        write_export_files(
            &all_data, &output_path, "user_roles",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Org Properties
    if options.org_properties {
        let all_data = fetch_iterative(
            client, &valid_ou_ids,
            |c, ou_id| Box::pin(c.get_org_properties(ou_id)),
            "Org Properties", &emit_progress, 80.0, 5.0, &mut warnings,
        ).await;

        write_export_files(
            &all_data, &output_path, "org_properties",
            export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
        );
    }

    // Device Properties (iterate filtered devices with bounded concurrency)
    if options.device_properties {
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
                    async move {
                        if idx % 10 == 0 {
                            tracing::info!("Device Properties: fetching {}/{}", idx, total_devices);
                        }
                        (device_id, client.get_device_properties(device_id).await)
                    }
                })
                .buffer_unordered(5)
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

            write_export_files(
                &all_props, &output_path, "device_properties",
                export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
            );

            // Re-cache for device_assets if needed
            if options.device_assets {
                cached_device_list = Some(devices_in_scope);
            }
        }
    }

    // Device Assets (iterate filtered devices with bounded concurrency)
    if options.device_assets {
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
                    async move {
                        if idx % 10 == 0 {
                            tracing::info!("Device Assets: fetching {}/{}", idx, total_devices);
                        }
                        (device_id, client.get_device_assets(device_id).await)
                    }
                })
                .buffer_unordered(5)
                .collect()
                .await;

            let mut all_assets = Vec::new();
            for (device_id, result) in results {
                match result {
                    Ok(asset) => all_assets.push(DeviceAssetFlat::from(asset)),
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

            write_export_files(
                &all_assets, &output_path, "device_assets",
                export_csv, export_json, &mut files_created, &mut total_records, &mut errors,
            );
        }
    }

    emit_progress("Complete", "Export finished", 100.0);

    let has_errors = !errors.is_empty();
    let message = if has_errors {
        format!(
            "Exported {} records to {} files with {} error(s)",
            total_records,
            files_created.len(),
            errors.len()
        )
    } else {
        format!(
            "Exported {} records to {} files",
            total_records,
            files_created.len()
        )
    };

    Ok(ExportResult {
        success: !has_errors || !files_created.is_empty(),
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
                .filter(|d| {
                    d.org_unit_id.map_or(false, |id| valid_ou_ids.contains(&id))
                        || d.customer_id.map_or(false, |id| valid_ou_ids.contains(&id))
                        || d.site_id.map_or(false, |id| valid_ou_ids.contains(&id))
                        || d.so_id.map_or(false, |id| valid_ou_ids.contains(&id))
                })
                .collect();
            tracing::info!("Devices filter: {} -> {}", initial, filtered.len());
            Ok(filtered)
        }
        Err(e) => Err(format!("Failed to fetch devices: {}", e)),
    }
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
        serde_json::json!({"id": "devices", "name": "Devices", "default": true}),
        serde_json::json!({"id": "access_groups", "name": "Access Groups", "default": true}),
        serde_json::json!({"id": "user_roles", "name": "User Roles", "default": true}),
        serde_json::json!({"id": "org_properties", "name": "Organization Properties", "default": true}),
        serde_json::json!({"id": "users", "name": "Users", "default": true}),
        serde_json::json!({"id": "device_properties", "name": "Device Properties", "default": false}),
        serde_json::json!({"id": "device_assets", "name": "Device Assets (Hardware)", "default": false}),
    ]
}

/// Cancel a running export or migration
#[tauri::command]
pub async fn cancel_export(state: State<'_, AppState>) -> std::result::Result<(), String> {
    state.cancel_token.store(true, std::sync::atomic::Ordering::Relaxed);
    tracing::info!("Export/migration cancellation requested");
    Ok(())
}
