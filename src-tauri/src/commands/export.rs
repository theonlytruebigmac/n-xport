//! Export-related Tauri commands

use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use tauri::{Emitter, State, Window};

use crate::commands::connection::AppState;
use crate::export::{export_to_csv, export_to_json};
use crate::models::{ExportOptions, ProgressUpdate};

/// Export result
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub success: bool,
    pub message: String,
    pub files_created: Vec<String>,
    pub total_records: usize,
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

    // Scan hierarchy if we need deep items
    let needs_hierarchy = options.sites
        || options.users
        || options.devices
        || options.access_groups
        || options.user_roles
        || options.org_properties
        || options.device_properties;

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
        Err(e) => tracing::error!("Failed to fetch target Service Org: {}", e),
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
            Err(e) => tracing::error!("Failed to fetch customers: {}", e),
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
                    // Site ID is typically the 'id' field, but let's check org_unit_id too
                    if let Some(oid) = s.org_unit_id {
                        valid_ou_ids.insert(oid);
                    }
                    // Also just in case 'site_id' or main 'id'
                    valid_ou_ids.insert(s.site_id);
                }
                fetched_sites = sites;
            }
            Err(e) => tracing::error!("Failed to fetch sites: {}", e),
        }
    }

    tracing::info!(
        "Hierarchy scan complete. Found {} valid Org Units.",
        valid_ou_ids.len()
    );

    // --- EXECUTE EXPORTS ---

    // Service Orgs
    if options.service_orgs && !fetched_service_orgs.is_empty() {
        if export_csv {
            let path = output_path.join("service_orgs.csv");
            if let Ok(c) = export_to_csv(&fetched_service_orgs, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
        if export_json {
            let path = output_path.join("service_orgs.json");
            if let Ok(c) = export_to_json(&fetched_service_orgs, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
    }

    // Customers
    if options.customers && !fetched_customers.is_empty() {
        if export_csv {
            let path = output_path.join("customers.csv");
            if let Ok(c) = export_to_csv(&fetched_customers, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
        if export_json {
            let path = output_path.join("customers.json");
            if let Ok(c) = export_to_json(&fetched_customers, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
    }

    // Sites
    if options.sites && !fetched_sites.is_empty() {
        if export_csv {
            let path = output_path.join("sites.csv");
            if let Ok(c) = export_to_csv(&fetched_sites, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
        if export_json {
            let path = output_path.join("sites.json");
            if let Ok(c) = export_to_json(&fetched_sites, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
    }

    // Users (GLOBAL FETCH + FILTER)
    if options.users {
        emit_progress("Users", "Fetching system-wide users...", 20.0);
        match client.get_users().await {
            Ok(all_users) => {
                let initial_count = all_users.len();
                let filtered_users: Vec<_> = all_users
                    .into_iter()
                    .filter(|u| {
                        // Check org_unit_id
                        if let Some(oid) = u.org_unit_id {
                            if valid_ou_ids.contains(&oid) {
                                return true;
                            }
                        }
                        // Check service_org_id (if strictly top level)
                        if let Some(sid) = u.service_org_id {
                            if valid_ou_ids.contains(&sid) {
                                return true;
                            }
                        }
                        false
                    })
                    .collect();

                tracing::info!(
                    "Users filter: {} -> {}",
                    initial_count,
                    filtered_users.len()
                );

                if export_csv {
                    let path = output_path.join("users.csv");
                    if let Ok(c) = export_to_csv(&filtered_users, &path) {
                        files_created.push(path.display().to_string());
                        total_records += c;
                    }
                }
                if export_json {
                    let path = output_path.join("users.json");
                    if let Ok(c) = export_to_json(&filtered_users, &path) {
                        files_created.push(path.display().to_string());
                        total_records += c;
                    }
                }
            }
            Err(e) => tracing::error!("Failed to fetch users: {}", e),
        }
    }

    // Devices (GLOBAL FETCH + FILTER)
    if options.devices {
        emit_progress("Devices", "Fetching system-wide devices...", 40.0);
        match client.get_devices().await {
            Ok(all_devices) => {
                let initial_count = all_devices.len();
                let filtered_devices: Vec<_> = all_devices
                    .into_iter()
                    .filter(|d| {
                        d.org_unit_id.map_or(false, |id| valid_ou_ids.contains(&id))
                            || d.customer_id.map_or(false, |id| valid_ou_ids.contains(&id))
                            || d.site_id.map_or(false, |id| valid_ou_ids.contains(&id))
                            || d.so_id.map_or(false, |id| valid_ou_ids.contains(&id))
                    })
                    .collect();

                tracing::info!(
                    "Devices filter: {} -> {}",
                    initial_count,
                    filtered_devices.len()
                );

                if export_csv {
                    let path = output_path.join("devices.csv");
                    if let Ok(c) = export_to_csv(&filtered_devices, &path) {
                        files_created.push(path.display().to_string());
                        total_records += c;
                    }
                }
                if export_json {
                    let path = output_path.join("devices.json");
                    if let Ok(c) = export_to_json(&filtered_devices, &path) {
                        files_created.push(path.display().to_string());
                        total_records += c;
                    }
                }
            }
            Err(e) => tracing::error!("Failed to fetch devices: {}", e),
        }
    }

    // --- ITERATIVE EXPORTS ---
    // For Access Groups, User Roles, Org Properties, we iterate valid OUs

    // Convert HashSet to Vec for iteration and sorting (deterministic order)
    let mut ou_list: Vec<i64> = valid_ou_ids.iter().cloned().collect();
    ou_list.sort();

    // Access Groups
    if options.access_groups {
        emit_progress("Access Groups", "Iterating Org Units...", 60.0);
        let mut all_data = Vec::new();
        for (idx, ou_id) in ou_list.iter().enumerate() {
            if idx % 10 == 0 {
                emit_progress(
                    "Access Groups",
                    &format!("Fetching {}/{}", idx, ou_list.len()),
                    60.0 + (idx as f32 / ou_list.len() as f32) * 5.0,
                );
            }
            if let Ok(items) = client.get_access_groups(*ou_id).await {
                all_data.extend(items);
            }
        }
        if export_csv {
            let path = output_path.join("access_groups.csv");
            if let Ok(c) = export_to_csv(&all_data, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
        if export_json {
            let path = output_path.join("access_groups.json");
            if let Ok(c) = export_to_json(&all_data, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
    }

    // User Roles
    if options.user_roles {
        emit_progress("User Roles", "Iterating Org Units...", 70.0);
        let mut all_data = Vec::new();
        for (idx, ou_id) in ou_list.iter().enumerate() {
            if idx % 10 == 0 {
                emit_progress(
                    "User Roles",
                    &format!("Fetching {}/{}", idx, ou_list.len()),
                    70.0 + (idx as f32 / ou_list.len() as f32) * 5.0,
                );
            }
            if let Ok(items) = client.get_user_roles(*ou_id).await {
                all_data.extend(items);
            }
        }
        if export_csv {
            let path = output_path.join("user_roles.csv");
            if let Ok(c) = export_to_csv(&all_data, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
        if export_json {
            let path = output_path.join("user_roles.json");
            if let Ok(c) = export_to_json(&all_data, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
    }

    // Org Properties
    if options.org_properties {
        emit_progress("Org Properties", "Iterating Org Units...", 80.0);
        let mut all_data = Vec::new();
        for (idx, ou_id) in ou_list.iter().enumerate() {
            if idx % 10 == 0 {
                emit_progress(
                    "Org Properties",
                    &format!("Fetching {}/{}", idx, ou_list.len()),
                    80.0 + (idx as f32 / ou_list.len() as f32) * 5.0,
                );
            }
            if let Ok(items) = client.get_org_properties(*ou_id).await {
                all_data.extend(items);
            }
        }
        if export_csv {
            let path = output_path.join("org_properties.csv");
            if let Ok(c) = export_to_csv(&all_data, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
        if export_json {
            let path = output_path.join("org_properties.json");
            if let Ok(c) = export_to_json(&all_data, &path) {
                files_created.push(path.display().to_string());
                total_records += c;
            }
        }
    }

    emit_progress("Complete", "Export finished", 100.0);

    Ok(ExportResult {
        success: true,
        message: format!(
            "Exported {} records to {} files (deep scan enabled)",
            total_records,
            files_created.len()
        ),
        files_created,
        total_records,
    })
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
    ]
}
