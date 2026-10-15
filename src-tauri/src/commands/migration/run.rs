//! The migration orchestration (`run_migration`) and the Tauri command that wraps it.

use super::access_groups::migrate_access_groups;
use super::customers::{map_existing_org_units, migrate_customers_and_sites};
use super::env::{emit_log, report_progress, MigrationEnv};
use super::properties::{migrate_device_properties, migrate_org_properties};
use super::roles::migrate_user_roles;
use super::summary::{build_migration_result, MigrationSummary};
use super::types::{IdMapping, MigrationOptions, MigrationResult};
use super::users::migrate_users;
use crate::commands::connection::AppState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Runtime, State};

/// Run a migration with the given environment. `start_migration` is a thin wrapper that
/// takes the connected clients out of the app state; this holds the orchestration so it
/// can also be driven directly (for example against live servers in tests).
pub(crate) async fn run_migration<R: Runtime>(
    env: &MigrationEnv<'_, R>,
    options: &MigrationOptions,
) -> std::result::Result<MigrationResult, String> {
    let MigrationEnv {
        source_so_id,
        dest_so_id,
        app_handle,
        cancel_token,
        ..
    } = *env;
    let mut mapping = IdMapping::new();
    let mut summary = MigrationSummary::default();

    // Always ensure the SO pair is in org_units, regardless of which options are selected.
    mapping.org_units.insert(source_so_id, dest_so_id);

    report_progress(app_handle, "Migration", "Starting migration engine...", 0.0);
    emit_log(app_handle, "info", "Starting migration engine...");

    // 1. Customers & Sites
    if options.customers {
        migrate_customers_and_sites(env, &mut mapping, &mut summary).await?;
    } else if options.needs_existing_org_unit_mapping() && !cancel_token.load(Ordering::Relaxed) {
        map_existing_org_units(env, &mut mapping, &mut summary).await?;
    }

    // 2. User Roles (at all org unit levels)
    if options.user_roles && !cancel_token.load(Ordering::Relaxed) {
        migrate_user_roles(env, &mut mapping, &mut summary).await?;
    }

    // 3. Users — must run BEFORE access groups so their IDs are available.
    // N-central has no API to update access group membership after creation,
    // so users must exist first and their IDs passed in the create payload.
    // Users are fetched from every level (SO, each customer, each site) and
    // created at the exact same org unit level they occupy on the source.
    if options.users && !cancel_token.load(Ordering::Relaxed) {
        migrate_users(env, &mut mapping, &mut summary).await?;
    }

    // 4. Access Groups (at all org unit levels — after users so user IDs are available)
    if options.access_groups && !cancel_token.load(Ordering::Relaxed) {
        migrate_access_groups(env, &mut mapping, &mut summary).await?;
        summary.note_memberless_access_groups(options);
    }

    // 5. Custom Properties
    if options.org_properties && !cancel_token.load(Ordering::Relaxed) {
        migrate_org_properties(env, &mapping, &mut summary).await?;
    }

    // 6. Device custom properties (only on existing devices matched by exact name
    // within each mapped org-unit scope).
    if options.device_properties && !cancel_token.load(Ordering::Relaxed) {
        migrate_device_properties(env, &mapping, &mut summary).await?;
    }

    let cancelled = cancel_token.load(Ordering::Relaxed);

    // ── Post-migration summary ──────────────────────────────────────────────────
    emit_log(
        app_handle,
        "info",
        "─────────────── Migration Summary ───────────────",
    );

    let issues = summary.issues();
    let manual_follow_up = summary.manual_follow_up();
    let has_issues = !issues.is_empty() || !manual_follow_up.is_empty();

    for issue in &summary.customers_failed {
        emit_log(app_handle, "error", issue);
    }
    for issue in &summary.sites_failed {
        emit_log(app_handle, "error", issue);
    }
    for issue in &summary.properties_failed {
        emit_log(app_handle, "error", issue);
    }
    for issue in &summary.fetch_failures {
        emit_log(app_handle, "warning", issue);
    }
    for item in &summary.properties_skipped {
        emit_log(app_handle, "warning", item);
    }

    if !summary.roles_failed.is_empty() {
        emit_log(
            app_handle,
            "warning",
            &format!(
                "Roles not created or mapped ({}): {}",
                summary.roles_failed.len(),
                summary.roles_failed.join(", ")
            ),
        );
    }

    if !summary.users_failed.is_empty() {
        emit_log(
            app_handle,
            "error",
            &format!(
                "Users failed to create ({}): {}",
                summary.users_failed.len(),
                summary.users_failed.join(", ")
            ),
        );
    }

    if !summary.users_no_roles.is_empty() {
        emit_log(app_handle, "warning", &format!(
            "Users created without some or all source roles ({}) — the role is a system role not available via API, or does not exist at the user's destination org unit:",
            summary.users_no_roles.len()
        ));
        for (login, roles) in &summary.users_no_roles {
            emit_log(
                app_handle,
                "warning",
                &format!("  • {} (source roles: {})", login, roles.join(", ")),
            );
        }
        emit_log(
            app_handle,
            "warning",
            "  → Assign roles manually via Administration > User Management > Users.",
        );
    }

    if !summary.access_groups_created.is_empty() {
        emit_log(
            app_handle,
            "success",
            &format!(
                "Access groups created with all destination users included ({}): {}",
                summary.access_groups_created.len(),
                summary.access_groups_created.join(", ")
            ),
        );
    }

    if !summary.access_groups_existed_not_updated.is_empty() {
        emit_log(
            app_handle,
            "warning",
            &format!(
                "Access groups already on destination — newly migrated users NOT added ({}):",
                summary.access_groups_existed_not_updated.len()
            ),
        );
        for name in &summary.access_groups_existed_not_updated {
            emit_log(app_handle, "warning", &format!("  • {}", name));
        }
        emit_log(
            app_handle,
            "warning",
            "  → N-central has no API to update existing group membership.",
        );
        emit_log(
            app_handle,
            "warning",
            "  → Add migrated users manually via Administration > Access Groups.",
        );
    }

    if !summary.access_groups_failed.is_empty() {
        emit_log(
            app_handle,
            "error",
            &format!(
                "Access groups failed to create ({}): {}",
                summary.access_groups_failed.len(),
                summary.access_groups_failed.join(", ")
            ),
        );
    }

    if cancelled {
        emit_log(
            app_handle,
            "warning",
            "Migration cancelled; completed changes were kept.",
        );
    } else if !has_issues {
        emit_log(
            app_handle,
            "success",
            "All items migrated successfully — no action required.",
        );
    }

    emit_log(
        app_handle,
        "info",
        "─────────────────────────────────────────────────",
    );
    // ────────────────────────────────────────────────────────────────────────────

    let result = build_migration_result(&summary, cancelled);
    report_progress(
        app_handle,
        if cancelled { "Cancelled" } else { "Complete" },
        &result.message,
        100.0,
    );

    Ok(result)
}

#[tauri::command]
pub async fn start_migration(
    options: MigrationOptions,
    source_so_id: i64,
    dest_so_id: i64,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<MigrationResult, String> {
    state.cancel_token.store(false, Ordering::Relaxed);
    let cancel_token = state.cancel_token.clone();

    let source_client = state.client.lock().await;
    let dest_client = state.dest_client.lock().await;

    let (source, dest) = match (&*source_client, &*dest_client) {
        (Some(s), Some(d)) => (s, d),
        _ => return Err("Both source and destination must be connected".to_string()),
    };

    // Get optional SOAP client for fallback operations
    let dest_soap = state.dest_soap_client.lock().await;
    let env = MigrationEnv {
        source,
        dest,
        source_so_id,
        dest_so_id,
        soap: dest_soap.as_ref(),
        app_handle: &app_handle,
        cancel_token: &cancel_token,
    };
    run_migration(&env, &options).await
}
