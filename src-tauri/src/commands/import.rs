//! Import-related Tauri commands.
//!
//! Mirrors export.rs in shape: a `start_import` command runs the work, emits
//! `import-progress` events for progress and `backend-log` events for per-row
//! outcomes, then returns an `ImportResult` summary. `generate_template` writes
//! a CSV template for any importable resource. `get_import_types` advertises
//! which resources are supported (and which are greyed out in the UI).

use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

use crate::commands::connection::{AppState, CachedImportContext};
use crate::config::PasswordPolicy;
use crate::import::handlers::{
    import_access_group, import_customer, import_site, import_user, import_user_role,
    ImportContext,
};
use crate::import::{
    read_rows, write_template, AccessGroupImportRow, CustomerImportRow, ImportResource,
    RowOutcome, RowStatus, SiteImportRow, UserImportRow, UserRoleImportRow,
};
use crate::models::{LogMessage, ProgressUpdate};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub success: bool,
    pub message: String,
    pub dry_run: bool,
    pub rows_total: usize,
    pub rows_created: usize,
    pub rows_skipped: usize,
    pub rows_errored: usize,
    pub rows_planned: usize,
    pub outcomes: Vec<RowOutcome>,
}

fn emit_progress(app: &AppHandle, phase: &str, message: &str, percent: f32) {
    let _ = app.emit(
        "import-progress",
        ProgressUpdate {
            phase: phase.to_string(),
            message: message.to_string(),
            percent,
            current: 0,
            total: 0,
        },
    );
}

fn emit_log(app: &AppHandle, level: &str, message: &str) {
    let _ = app.emit(
        "backend-log",
        LogMessage {
            level: level.to_string(),
            message: message.to_string(),
        },
    );
}

fn outcome_log_level(status: RowStatus) -> &'static str {
    match status {
        RowStatus::Created => "success",
        RowStatus::Skipped => "info",
        RowStatus::Error => "error",
        RowStatus::Planned => "info",
    }
}

/// Generate a CSV template for the given resource type. Writes to `path`.
#[tauri::command]
pub async fn generate_template(resource_type: String, path: String) -> Result<String, String> {
    let resource = ImportResource::from_id(&resource_type)
        .ok_or_else(|| format!("Unknown resource type: {}", resource_type))?;
    let path_buf = PathBuf::from(path);
    write_template(resource, &path_buf).map_err(|e| format!("Failed to write template: {}", e))?;
    Ok(path_buf.display().to_string())
}

/// List importable resource types (and which ones are greyed out in the UI).
#[tauri::command]
pub fn get_import_types() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"id": "service_orgs", "name": "Service Organizations", "supported": false}),
        serde_json::json!({"id": "customers", "name": "Customers", "supported": true}),
        serde_json::json!({"id": "sites", "name": "Sites", "supported": true}),
        serde_json::json!({"id": "devices", "name": "Devices", "supported": false}),
        serde_json::json!({"id": "access_groups", "name": "Access Groups", "supported": true}),
        serde_json::json!({"id": "user_roles", "name": "User Roles", "supported": true}),
        serde_json::json!({"id": "org_properties", "name": "Organization Properties", "supported": false}),
        serde_json::json!({"id": "users", "name": "Users", "supported": true}),
        serde_json::json!({"id": "device_properties", "name": "Device Properties", "supported": false}),
        serde_json::json!({"id": "device_assets", "name": "Device Assets (Hardware)", "supported": false}),
    ]
}

/// Run a CSV import against the source-connected N-central server.
#[tauri::command]
pub async fn start_import(
    app_handle: AppHandle,
    resource_type: String,
    csv_path: String,
    service_org_id: i64,
    dry_run: bool,
    password_policy: Option<PasswordPolicy>,
    state: State<'_, AppState>,
) -> Result<ImportResult, String> {
    let resource = ImportResource::from_id(&resource_type)
        .ok_or_else(|| format!("Unknown / unsupported resource type: {}", resource_type))?;

    let client_guard = state.client.lock().await;
    let client = match &*client_guard {
        Some(c) => c.clone(),
        None => return Err("Not connected".to_string()),
    };
    drop(client_guard);

    let soap_guard = state.source_soap_client.lock().await;
    let soap = soap_guard.as_ref();

    // Reuse a cached ImportContext from a previous run against the same SO if
    // present — avoids a full re-fetch of customers/sites/roles/groups when the
    // user clicks "Apply for real" right after a successful dry-run.
    let mut ctx = {
        let mut cache = state.import_context_cache.lock().await;
        let cached_matches = cache
            .as_ref()
            .map(|c| c.service_org_id == service_org_id)
            .unwrap_or(false);
        if cached_matches {
            emit_progress(&app_handle, "Loading", "Reusing cached name lookups...", 5.0);
            emit_log(&app_handle, "info", "Reusing name lookups from previous run");
            cache.take().expect("checked above").ctx
        } else {
            emit_progress(&app_handle, "Loading", "Building name lookups...", 5.0);
            ImportContext::load(&client, service_org_id)
                .await
                .map_err(|e| format!("Failed to load lookup tables: {}", e))?
        }
    };

    emit_progress(&app_handle, "Parsing", "Reading CSV...", 10.0);
    let path = PathBuf::from(&csv_path);

    let mut outcomes: Vec<RowOutcome> = Vec::new();
    let phase = if dry_run { "Dry-run" } else { "Importing" };

    let report = |outcome: &RowOutcome| {
        emit_log(
            &app_handle,
            outcome_log_level(outcome.status),
            &format!(
                "Row {} [{}] {} — {}",
                outcome.row_number,
                match outcome.status {
                    RowStatus::Created => "CREATED",
                    RowStatus::Skipped => "SKIPPED",
                    RowStatus::Error => "ERROR",
                    RowStatus::Planned => "PLANNED",
                },
                outcome.label,
                outcome.message
            ),
        );
    };

    match resource {
        ImportResource::Customers => {
            let rows: Vec<CustomerImportRow> = read_rows(&path)
                .map_err(|e| format!("CSV parse failed: {}", e))?;
            let total = rows.len();
            if total == 0 {
                emit_log(&app_handle, "warning", "CSV had no data rows");
            }
            for (i, row) in rows.into_iter().enumerate() {
                let row_number = i + 2;
                let pct = 10.0 + ((i as f32 / total.max(1) as f32) * 85.0);
                emit_progress(&app_handle, phase, &format!("Row {}/{}", i + 1, total), pct);
                let outcome =
                    import_customer(row_number, row, &mut ctx, &client, soap, dry_run, &app_handle)
                        .await;
                report(&outcome);
                outcomes.push(outcome);
            }
        }
        ImportResource::Sites => {
            let rows: Vec<SiteImportRow> = read_rows(&path)
                .map_err(|e| format!("CSV parse failed: {}", e))?;
            let total = rows.len();
            if total == 0 {
                emit_log(&app_handle, "warning", "CSV had no data rows");
            }
            for (i, row) in rows.into_iter().enumerate() {
                let row_number = i + 2;
                let pct = 10.0 + ((i as f32 / total.max(1) as f32) * 85.0);
                emit_progress(&app_handle, phase, &format!("Row {}/{}", i + 1, total), pct);
                let outcome =
                    import_site(row_number, row, &mut ctx, &client, dry_run, &app_handle).await;
                report(&outcome);
                outcomes.push(outcome);
            }
        }
        ImportResource::AccessGroups => {
            let rows: Vec<AccessGroupImportRow> = read_rows(&path)
                .map_err(|e| format!("CSV parse failed: {}", e))?;
            let total = rows.len();
            if total == 0 {
                emit_log(&app_handle, "warning", "CSV had no data rows");
            }
            for (i, row) in rows.into_iter().enumerate() {
                let row_number = i + 2;
                let pct = 10.0 + ((i as f32 / total.max(1) as f32) * 85.0);
                emit_progress(&app_handle, phase, &format!("Row {}/{}", i + 1, total), pct);
                let outcome = import_access_group(
                    row_number,
                    row,
                    &mut ctx,
                    &client,
                    soap,
                    dry_run,
                    &app_handle,
                )
                .await;
                report(&outcome);
                outcomes.push(outcome);
            }
        }
        ImportResource::UserRoles => {
            let rows: Vec<UserRoleImportRow> = read_rows(&path)
                .map_err(|e| format!("CSV parse failed: {}", e))?;
            let total = rows.len();
            if total == 0 {
                emit_log(&app_handle, "warning", "CSV had no data rows");
            }
            for (i, row) in rows.into_iter().enumerate() {
                let row_number = i + 2;
                let pct = 10.0 + ((i as f32 / total.max(1) as f32) * 85.0);
                emit_progress(&app_handle, phase, &format!("Row {}/{}", i + 1, total), pct);
                let outcome = import_user_role(
                    row_number,
                    row,
                    &mut ctx,
                    &client,
                    soap,
                    dry_run,
                    &app_handle,
                )
                .await;
                report(&outcome);
                outcomes.push(outcome);
            }
        }
        ImportResource::Users => {
            let rows: Vec<UserImportRow> = read_rows(&path)
                .map_err(|e| format!("CSV parse failed: {}", e))?;
            let total = rows.len();
            if total == 0 {
                emit_log(&app_handle, "warning", "CSV had no data rows");
            }
            for (i, row) in rows.into_iter().enumerate() {
                let row_number = i + 2;
                let pct = 10.0 + ((i as f32 / total.max(1) as f32) * 85.0);
                emit_progress(&app_handle, phase, &format!("Row {}/{}", i + 1, total), pct);
                let outcome = import_user(
                    row_number,
                    row,
                    &mut ctx,
                    soap,
                    dry_run,
                    password_policy.as_ref(),
                    &app_handle,
                )
                .await;
                report(&outcome);
                outcomes.push(outcome);
            }
        }
    }

    // Store the (possibly mutated) ctx back so the next start_import call for
    // the same SO can reuse it. Mutations from live runs (e.g. newly-created
    // customer IDs registered into the lookup) are persisted intentionally.
    {
        let mut cache = state.import_context_cache.lock().await;
        *cache = Some(CachedImportContext { service_org_id, ctx });
    }

    let mut created = 0;
    let mut skipped = 0;
    let mut errored = 0;
    let mut planned = 0;
    for o in &outcomes {
        match o.status {
            RowStatus::Created => created += 1,
            RowStatus::Skipped => skipped += 1,
            RowStatus::Error => errored += 1,
            RowStatus::Planned => planned += 1,
        }
    }
    let total = outcomes.len();

    emit_progress(&app_handle, "Complete", "Import finished", 100.0);

    let message = if dry_run {
        format!(
            "Dry-run finished: {} planned, {} would skip, {} would error (of {} rows)",
            planned, skipped, errored, total
        )
    } else {
        format!(
            "Import finished: {} created, {} skipped, {} errored (of {} rows)",
            created, skipped, errored, total
        )
    };

    Ok(ImportResult {
        success: errored == 0,
        message,
        dry_run,
        rows_total: total,
        rows_created: created,
        rows_skipped: skipped,
        rows_errored: errored,
        rows_planned: planned,
        outcomes,
    })
}
