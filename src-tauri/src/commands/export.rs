//! Export-related Tauri commands

use std::path::PathBuf;
use tauri::{State, Window, Emitter};
use serde::Serialize;
use std::collections::HashSet;

use crate::commands::connection::AppState;
use crate::models::{ExportOptions, ProgressUpdate};
use crate::export::{export_to_csv, export_to_json};

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
    open::that(abs_path)
        .map_err(|e| format!("Failed to open directory: {}", e))?;
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
    // Store customer IDs for filtering sites
    let mut customer_ids: HashSet<i64> = HashSet::new();

    // Helper to emit progress
    let emit_progress = |phase: &str, message: &str, percent: f32| {
        let _ = window.emit("export-progress", ProgressUpdate {
            phase: phase.to_string(),
            message: message.to_string(),
            percent,
            current: 0,
            total: 0,
        });
    };

    // Export Service Orgs (just the target one)
    if options.service_orgs {
        emit_progress("Service Organizations", "Fetching...", 5.0);
        
        match client.get_service_org_by_id(service_org_id).await {
            Ok(data) => {
                let data_vec = vec![data];
                if export_csv {
                    let path = output_path.join("service_orgs.csv");
                    match export_to_csv(&data_vec, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export service_orgs.csv: {}", e),
                    }
                }
                if export_json {
                    let path = output_path.join("service_orgs.json");
                    match export_to_json(&data_vec, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export service_orgs.json: {}", e),
                    }
                }
                tracing::info!("Finished exporting Service Orgs");
            }
            Err(e) => tracing::error!("Failed to export service orgs: {}", e),
        }
    }

    // Export Customers
    if options.customers {
        emit_progress("Customers", "Fetching...", 15.0);
        
        match client.get_customers_by_so(service_org_id).await {
            Ok(data) => {
                // Store IDs for filtering
                for c in &data {
                    customer_ids.insert(c.customer_id);
                }
                
                if export_csv {
                    let path = output_path.join("customers.csv");
                    match export_to_csv(&data, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export customers.csv: {}", e),
                    }
                }
                if export_json {
                    let path = output_path.join("customers.json");
                    match export_to_json(&data, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export customers.json: {}", e),
                    }
                }
                tracing::info!("Finished exporting Customers");
            }
            Err(e) => tracing::error!("Failed to export customers: {}", e),
        }
    }

    // Export Sites (under target SO)
    if options.sites {
        emit_progress("Sites", "Fetching...", 25.0);
        
        // Ensure we have customer IDs for filtering if we didn't fetch them above
        if customer_ids.is_empty() {
            // Also include the SO ID itself as a potential parent, just in case
            customer_ids.insert(service_org_id);
            
            if let Ok(cust_data) = client.get_customers_by_so(service_org_id).await {
                for c in &cust_data {
                    customer_ids.insert(c.customer_id);
                }
            }
        } else {
             customer_ids.insert(service_org_id);
        }
        
        // We use get_sites_by_so which now hits /api/sites (all sites), so we MUST filter
        match client.get_sites_by_so(service_org_id).await {
            Ok(mut data) => {
                let initial_count = data.len();
                // Filter sites that belong to one of our customers (or the SO itself)
                data.retain(|s| {
                    let pid_match = s.parent_id.map_or(false, |pid| customer_ids.contains(&pid));
                    let cid_match = s.customer_id.map_or(false, |cid| customer_ids.contains(&cid));
                    let cid2_match = s.customerid.map_or(false, |cid| customer_ids.contains(&cid));
                    let oid_match = s.org_unit_id.map_or(false, |oid| customer_ids.contains(&oid));
                    let sid_match = s.service_org_id.map_or(false, |sid| customer_ids.contains(&sid));
                    let sid2_match = s.service_orgid.map_or(false, |sid| customer_ids.contains(&sid));
                    pid_match || cid_match || cid2_match || oid_match || sid_match || sid2_match
                });
                
                if initial_count > data.len() {
                    tracing::info!("Filtered sites from {} to {} for SO {}", initial_count, data.len(), service_org_id);
                }
                
                if initial_count > 0 && data.is_empty() {
                    tracing::warn!("Site filtering resulted in 0 records. Reviewing parent/linkage IDs...");
                    // Try to log the first few sites' ID fields to see why they didn't match
                    // Since data.retain already filtered it, we'd need to fetch again or peek earlier.
                    // Instead, I'll log the target customer_ids we are checking against.
                    tracing::info!("Target IDs for SO {}: {:?}", service_org_id, customer_ids);
                }

                if export_csv {
                    let path = output_path.join("sites.csv");
                    match export_to_csv(&data, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export sites.csv: {}", e),
                    }
                }
                if export_json {
                    let path = output_path.join("sites.json");
                    match export_to_json(&data, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export sites.json: {}", e),
                    }
                }
                tracing::info!("Finished exporting Sites");
            }
            Err(e) => tracing::error!("Failed to export sites: {}", e),
        }
    }

    // Export Devices (under target SO)
    if options.devices {
        emit_progress("Devices", "Fetching...", 35.0);
        
        match client.get_devices_by_org_unit(service_org_id).await {
            Ok(data) => {
                if export_csv {
                    let path = output_path.join("devices.csv");
                    match export_to_csv(&data, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export devices.csv: {}", e),
                    }
                }
                if export_json {
                    let path = output_path.join("devices.json");
                    match export_to_json(&data, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export devices.json: {}", e),
                    }
                }
            }
            Err(e) => tracing::error!("Failed to export devices: {}", e),
        }
    }

    // Export Access Groups
    if options.access_groups {
        emit_progress("Access Groups", "Fetching...", 55.0);
        
        match client.get_access_groups(service_org_id).await {
            Ok(data) => {
                if export_csv {
                    let path = output_path.join("access_groups.csv");
                    if let Ok(count) = export_to_csv(&data, &path) {
                        files_created.push(path.display().to_string());
                        total_records += count;
                    }
                }
                if export_json {
                    let path = output_path.join("access_groups.json");
                    if let Ok(count) = export_to_json(&data, &path) {
                        files_created.push(path.display().to_string());
                        total_records += count;
                    }
                }
            }
            Err(e) => tracing::error!("Failed to export access groups: {}", e),
        }
    }

    // Export User Roles
    if options.user_roles {
        emit_progress("User Roles", "Fetching...", 65.0);
        
        match client.get_user_roles(service_org_id).await {
            Ok(data) => {
                if export_csv {
                    let path = output_path.join("user_roles.csv");
                    if let Ok(count) = export_to_csv(&data, &path) {
                        files_created.push(path.display().to_string());
                        total_records += count;
                    }
                }
                if export_json {
                    let path = output_path.join("user_roles.json");
                    if let Ok(count) = export_to_json(&data, &path) {
                        files_created.push(path.display().to_string());
                        total_records += count;
                    }
                }
            }
            Err(e) => tracing::error!("Failed to export user roles: {}", e),
        }
    }

    // Export Org Properties
    if options.org_properties {
        emit_progress("Organization Properties", "Fetching...", 75.0);
        
        match client.get_org_properties(service_org_id).await {
            Ok(data) => {
                if export_csv {
                    let path = output_path.join("org_properties.csv");
                    if let Ok(count) = export_to_csv(&data, &path) {
                        files_created.push(path.display().to_string());
                        total_records += count;
                    }
                }
                if export_json {
                    let path = output_path.join("org_properties.json");
                    if let Ok(count) = export_to_json(&data, &path) {
                        files_created.push(path.display().to_string());
                        total_records += count;
                    }
                }
            }
            Err(e) => tracing::error!("Failed to export org properties: {}", e),
        }
    }

    // Export Users
    if options.users {
        emit_progress("Users", "Fetching...", 45.0);
        
        match client.get_users_by_org_unit(service_org_id).await {
            Ok(data) => {
                if export_csv {
                    let path = output_path.join("users.csv");
                    match export_to_csv(&data, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export users.csv: {}", e),
                    }
                }
                if export_json {
                    let path = output_path.join("users.json");
                    match export_to_json(&data, &path) {
                        Ok(count) => {
                            files_created.push(path.display().to_string());
                            total_records += count;
                        }
                        Err(e) => tracing::error!("Failed to export users.json: {}", e),
                    }
                }
                tracing::info!("Finished exporting Users");
            }
            Err(e) => tracing::error!("Failed to export users: {}", e),
        }
    }

    emit_progress("Complete", "Export finished", 100.0);

    Ok(ExportResult {
        success: true,
        message: format!("Exported {} records to {} files", total_records, files_created.len()),
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
