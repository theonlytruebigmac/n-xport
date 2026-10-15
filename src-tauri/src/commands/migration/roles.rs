//! User roles, across the service org, customers, and sites.

use super::env::{emit_log, fetch_or_warn, report_progress, MigrationEnv};
use super::summary::MigrationSummary;
use super::types::IdMapping;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::Runtime;

/// Migrate user roles from source to destination across all org unit levels.
/// Populates `mapping.roles` and `mapping.role_names`.
pub(super) async fn migrate_user_roles<R: Runtime>(
    env: &MigrationEnv<'_, R>,
    mapping: &mut IdMapping,
    summary: &mut MigrationSummary,
) -> Result<(), String> {
    let MigrationEnv {
        source,
        dest,
        source_so_id,
        soap: soap_client,
        app_handle,
        cancel_token,
        ..
    } = *env;
    if cancel_token.load(Ordering::Relaxed) {
        return Ok(());
    }
    report_progress(app_handle, "Roles", "Loading permission mappings...", 28.0);

    let perm_lookup = crate::models::user_role::PermissionLookup::bundled();

    if perm_lookup.is_empty() {
        tracing::warn!("No permission mappings loaded from CSV - roles will be created with minimal permissions");
        emit_log(
            app_handle,
            "warning",
            "No permission mappings loaded — roles will have minimal permissions",
        );
    }

    // Build ordered org unit pairs: SO first, then customers, then sites.
    // This ensures parent-level roles are processed before children, so inherited
    // roles (which N-central returns at every child OU) are recognized as duplicates.
    let mut org_unit_pairs: Vec<(i64, i64)> = Vec::new();
    // SO first
    if let Some(&dest_so) = mapping.org_units.get(&source_so_id) {
        org_unit_pairs.push((source_so_id, dest_so));
    }
    // Then customers
    for (&src, &dst) in &mapping.customers {
        org_unit_pairs.push((src, dst));
    }
    // Then sites
    for (&src, &dst) in &mapping.sites {
        org_unit_pairs.push((src, dst));
    }
    let total_ous = org_unit_pairs.len();

    emit_log(
        app_handle,
        "info",
        &format!("Roles: Scanning {} org units for roles...", total_ous),
    );

    // A role is listed at every org unit beneath the one that defines it, with the same ID.
    // Visit each source role once, at the first (highest) org unit that lists it.
    let mut seen_source_roles: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for (idx, &(src_ou, dest_ou)) in org_unit_pairs.iter().enumerate() {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        let progress = 28.0 + (idx as f32 / total_ous as f32) * 12.0;
        let ou_label = if src_ou == source_so_id {
            "Service Org".to_string()
        } else if mapping.customers.contains_key(&src_ou) {
            format!("Customer {}", src_ou)
        } else {
            format!("Site {}", src_ou)
        };

        report_progress(
            app_handle,
            "Roles",
            &format!("Fetching roles from {} ...", ou_label),
            progress,
        );

        let source_roles = fetch_or_warn(
            source.get_user_roles(src_ou).await,
            app_handle,
            &format!("source roles at {}", ou_label),
            &mut summary.fetch_failures,
        );
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        if source_roles.is_empty() {
            continue;
        }

        let fetch_failures_before_dest_roles = summary.fetch_failures.len();
        let dest_roles = fetch_or_warn(
            dest.get_user_roles(dest_ou).await,
            app_handle,
            &format!("dest roles at {}", ou_label),
            &mut summary.fetch_failures,
        );
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        if summary.fetch_failures.len() != fetch_failures_before_dest_roles {
            continue;
        }
        let mut dest_name_map: HashMap<String, i64> = HashMap::new();
        for r in &dest_roles {
            if let Some(ref name) = r.role_name {
                dest_name_map.insert(name.to_lowercase(), r.role_id);
            }
        }

        for source_role in source_roles {
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }
            if !seen_source_roles.insert(source_role.role_id) {
                continue;
            }
            let role_name = source_role.role_name.as_deref().unwrap_or("Unknown Role");
            let role_name_lower = role_name.to_lowercase();

            // Check if this role already exists (or is inherited) at the destination OU.
            // We rely on dest_name_map (fetched per-OU) rather than the global mapping.role_names,
            // because same-named roles at sibling OUs (e.g., "Demo - Customer Manager" at two
            // different customers) are distinct roles, not inherited duplicates.
            if let Some(&id) = dest_name_map.get(&role_name_lower) {
                mapping.roles.insert(source_role.role_id, id);
                mapping.role_names.insert(role_name_lower, id);
                let msg = format!(
                    "Role '{}' already exists at dest {} (ID: {})",
                    role_name, ou_label, id
                );
                tracing::debug!("{}", msg);
                emit_log(app_handle, "debug", &msg);
                summary.skipped.push(msg);
            } else {
                if cancel_token.load(Ordering::Relaxed) {
                    break;
                }
                let source_permissions = source_role.get_permissions();
                let mut permission_ids: Vec<i64> =
                    if !perm_lookup.is_empty() && !source_permissions.is_empty() {
                        perm_lookup.names_to_ids(&source_permissions)
                    } else {
                        vec![]
                    };
                // N-central requires at least one permission to create a role.
                // If we couldn't resolve any from the source, use a minimal fallback
                // so the role still exists for user assignment.
                if permission_ids.is_empty() {
                    permission_ids = vec![1701]; // ACTIVE_ISSUES_VIEW
                    emit_log(
                        app_handle,
                        "debug",
                        &format!(
                            "Role '{}' at {} — permissions not available from source API, using minimal fallback",
                            role_name, ou_label
                        ),
                    );
                }

                let description = source_role
                    .role_description
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Migrated role");

                let permission_id_strings: Vec<String> =
                    permission_ids.iter().map(ToString::to_string).collect();
                let payload = serde_json::json!({
                    "roleName": role_name,
                    "description": description,
                    "permissionIds": permission_id_strings,
                    "userIds": []
                });

                tracing::info!(
                    "Creating role '{}' at {} with {} permissions...",
                    role_name,
                    ou_label,
                    permission_ids.len()
                );

                // For customer/site-level roles, prefer SOAP userRoleAdd which respects the
                // customerID parameter for OU-level placement. The REST API always creates
                // roles at the SO level regardless of the orgUnitId in the URL.
                let is_so_level = src_ou == source_so_id;
                let mut created = false;

                if !is_so_level {
                    // Try SOAP first for non-SO roles (ensures the role exists on the server).
                    // Do NOT store the SOAP-returned ID in mapping.roles — both SOAP and REST
                    // return a global/SO-level ID, not the per-OU inherited view ID that
                    // N-central requires for user role assignment. The re-fetch after this
                    // loop will find the correct per-OU ID.
                    if let Some(soap) = soap_client {
                        match soap
                            .user_role_add(role_name, description, dest_ou, &permission_ids)
                            .await
                        {
                            Ok(id) if id > 0 => {
                                let msg = format!("Created role '{}' at {} via SOAP (ID: {}), re-fetch will resolve per-OU ID", role_name, ou_label, id);
                                tracing::info!("{}", msg);
                                emit_log(app_handle, "success", &msg);
                                created = true;
                            }
                            Ok(_) => {
                                emit_log(app_handle, "debug", &format!(
                                    "SOAP userRoleAdd '{}' at {} returned non-positive ID, will try REST + re-fetch",
                                    role_name, ou_label
                                ));
                            }
                            Err(e) => {
                                emit_log(app_handle, "debug", &format!(
                                    "SOAP userRoleAdd '{}' at {} failed: {}, will try REST + re-fetch",
                                    role_name, ou_label, e
                                ));
                            }
                        }
                    }
                }

                if !created && !cancel_token.load(Ordering::Relaxed) {
                    // REST fallback (works well for SO-level roles; for others, re-fetch picks up the ID)
                    match dest.create_user_role(dest_ou, &payload).await {
                        Ok(resp) => {
                            let data = if resp.get("data").is_some() {
                                &resp["data"]
                            } else {
                                &resp
                            };
                            let id = data["roleId"]
                                .as_i64()
                                .or_else(|| data["userRoleId"].as_i64())
                                .or_else(|| data["id"].as_i64())
                                .or_else(|| resp["roleId"].as_i64())
                                .or_else(|| resp["id"].as_i64())
                                .unwrap_or(0);
                            if id > 0 && is_so_level {
                                // Only trust REST IDs for SO-level roles (REST always creates at SO)
                                mapping.roles.insert(source_role.role_id, id);
                                mapping.role_names.insert(role_name_lower.clone(), id);
                                let msg = format!(
                                    "Created role '{}' at {} (ID: {}, {} permissions)",
                                    role_name,
                                    ou_label,
                                    id,
                                    permission_ids.len()
                                );
                                tracing::info!("{}", msg);
                                emit_log(app_handle, "success", &msg);
                                let _ = created; // suppress unused warning
                            } else {
                                emit_log(app_handle, "debug", &format!(
                                    "REST created role '{}' at {} (resp ID: {}), will verify via re-fetch",
                                    role_name, ou_label, id
                                ));
                            }
                        }
                        Err(rest_err) => {
                            emit_log(
                                app_handle,
                                "debug",
                                &format!(
                                    "REST create_user_role '{}' at {} failed: {}",
                                    role_name, ou_label, rest_err
                                ),
                            );
                        }
                    }
                }
            }
        }

        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        // Safety net: re-fetch dest roles to pick up any that were created but whose
        // IDs weren't captured from the create response. Match source roles by name.
        let refreshed_dest_roles = fetch_or_warn(
            dest.get_user_roles(dest_ou).await,
            app_handle,
            &format!("dest roles re-fetch at {}", ou_label),
            &mut summary.fetch_failures,
        );
        let mut refreshed_name_map: HashMap<String, i64> = HashMap::new();
        for r in &refreshed_dest_roles {
            if let Some(ref name) = r.role_name {
                refreshed_name_map.insert(name.to_lowercase(), r.role_id);
            }
        }

        // Re-fetch source roles to fill in any mapping gaps
        let source_roles_refetch = fetch_or_warn(
            source.get_user_roles(src_ou).await,
            app_handle,
            &format!("source roles re-fetch at {}", ou_label),
            &mut summary.fetch_failures,
        );
        for source_role in &source_roles_refetch {
            if mapping.roles.contains_key(&source_role.role_id) {
                continue; // Already mapped
            }
            let role_name = source_role.role_name.as_deref().unwrap_or("");
            if let Some(&dest_id) = refreshed_name_map.get(&role_name.to_lowercase()) {
                mapping.roles.insert(source_role.role_id, dest_id);
                mapping.role_names.insert(role_name.to_lowercase(), dest_id);
                emit_log(
                    app_handle,
                    "debug",
                    &format!(
                        "Role '{}' at {} mapped via re-fetch (src {} → dest {})",
                        role_name, ou_label, source_role.role_id, dest_id
                    ),
                );
            } else {
                // Role exists on source but neither create nor re-fetch found it on dest.
                summary
                    .roles_failed
                    .push(format!("{} (at {})", role_name, ou_label));
            }
        }
    }

    emit_log(
        app_handle,
        "info",
        &format!(
            "Roles: {} total role mappings across {} org units",
            mapping.roles.len(),
            total_ous
        ),
    );

    Ok(())
}
