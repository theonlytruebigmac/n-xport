//! Users, created through SOAP at the same org-unit level they have on the source.

use super::env::{emit_log, fetch_or_warn, report_progress, MigrationEnv};
use super::mapping::resolve_destination_role;
use super::summary::MigrationSummary;
use super::types::IdMapping;
use crate::api::UserAddInfo;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::Runtime;

/// Migrate users from source to destination via SOAP API across all org unit levels.
/// Fetches users from each org unit (SO, customers, sites) and creates them at the
/// matching destination org unit. Populates `mapping.user_logins`.
pub(super) async fn migrate_users<R: Runtime>(
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
    // Build a complete source role_id -> role_name map across ALL org units
    report_progress(
        app_handle,
        "Users",
        "Fetching source users and roles...",
        70.0,
    );
    let mut source_role_id_to_name: HashMap<i64, String> = HashMap::new();
    for &src_ou in mapping.org_units.keys() {
        if cancel_token.load(Ordering::Relaxed) {
            return Ok(());
        }
        let roles = fetch_or_warn(
            source.get_user_roles(src_ou).await,
            app_handle,
            &format!("source roles for OU {} (users phase)", src_ou),
            &mut summary.fetch_failures,
        );
        for r in roles {
            if let Some(name) = r.role_name {
                source_role_id_to_name.insert(r.role_id, name.to_lowercase());
            }
        }
    }

    // If role_names map is empty (roles weren't migrated), populate from all dest org units
    if mapping.role_names.is_empty() {
        tracing::info!("Role names map is empty - fetching destination roles to populate...");
        for &dest_ou in mapping.org_units.values() {
            if cancel_token.load(Ordering::Relaxed) {
                return Ok(());
            }
            let dest_roles = fetch_or_warn(
                dest.get_user_roles(dest_ou).await,
                app_handle,
                &format!("dest roles for OU {} (populate role_names)", dest_ou),
                &mut summary.fetch_failures,
            );
            for r in dest_roles {
                if let Some(name) = r.role_name {
                    mapping.role_names.insert(name.to_lowercase(), r.role_id);
                }
            }
        }
        tracing::info!(
            "Populated role_names from destination with {} roles",
            mapping.role_names.len()
        );
    }

    let msg = format!(
        "Users: Built source role ID->name map with {} entries. Dest role_names map has {} entries.",
        source_role_id_to_name.len(),
        mapping.role_names.len()
    );
    tracing::info!("{}", msg);
    emit_log(app_handle, "debug", &msg);

    // Build a global dest login map across all org units (to skip existing users)
    report_progress(app_handle, "Users", "Fetching destination users...", 73.0);
    let mut dest_login_map: HashMap<String, i64> = HashMap::new();
    let fetch_failures_before_dest_users = summary.fetch_failures.len();
    for &dest_ou in mapping.org_units.values() {
        if cancel_token.load(Ordering::Relaxed) {
            return Ok(());
        }
        let dest_users = fetch_or_warn(
            dest.get_users_by_org_unit(dest_ou).await,
            app_handle,
            &format!("dest users for OU {}", dest_ou),
            &mut summary.fetch_failures,
        );
        if summary.fetch_failures.len() != fetch_failures_before_dest_users {
            return Ok(());
        }
        for u in dest_users {
            dest_login_map.insert(u.login_name.to_lowercase(), u.user_id);
        }
    }

    // Fetch users from every source org unit, ordered SO → customers → sites.
    // Processing top-down ensures that if a user appears at multiple levels
    // (e.g., inherited), we pick the most specific (deepest) level via dedup.
    // Actually we reverse: sites first so the deepest OU wins in dedup.
    let mut org_unit_pairs: Vec<(i64, i64)> = Vec::new();
    // Sites first (most specific)
    for (&src, &dst) in &mapping.sites {
        org_unit_pairs.push((src, dst));
    }
    // Then customers
    for (&src, &dst) in &mapping.customers {
        org_unit_pairs.push((src, dst));
    }
    // SO last (least specific) — dedup will skip users already seen at deeper levels
    if let Some(&dest_so) = mapping.org_units.get(&source_so_id) {
        org_unit_pairs.push((source_so_id, dest_so));
    }
    let total_ous = org_unit_pairs.len();
    let mut all_source_users: Vec<(crate::models::User, i64)> = Vec::new(); // (user, dest_ou_id)
    let mut seen_logins: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (idx, &(src_ou, dest_ou)) in org_unit_pairs.iter().enumerate() {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        let progress = 73.0 + (idx as f32 / total_ous as f32) * 4.0;
        let ou_label = if src_ou == source_so_id {
            "Service Org".to_string()
        } else if mapping.customers.contains_key(&src_ou) {
            format!("Customer {}", src_ou)
        } else {
            format!("Site {}", src_ou)
        };

        report_progress(
            app_handle,
            "Users",
            &format!("Fetching users from {} ...", ou_label),
            progress,
        );

        let users = fetch_or_warn(
            source.get_users_by_org_unit(src_ou).await,
            app_handle,
            &format!("source users at {}", ou_label),
            &mut summary.fetch_failures,
        );
        for user in users {
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }
            let login_lower = user.login_name.to_lowercase();
            if seen_logins.contains(&login_lower) {
                continue; // Skip duplicates (user may appear at parent OU too)
            }
            seen_logins.insert(login_lower);
            all_source_users.push((user, dest_ou));
        }
    }

    emit_log(
        app_handle,
        "info",
        &format!(
            "Users: Found {} unique users across {} org units",
            all_source_users.len(),
            total_ous
        ),
    );

    // Create users
    let mut dest_roles_by_ou: HashMap<i64, Option<HashMap<String, i64>>> = HashMap::new();
    let total = all_source_users.len();
    for (i, (source_user, dest_ou)) in all_source_users.into_iter().enumerate() {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        let progress = 77.0 + (i as f32 / total as f32) * 13.0;
        report_progress(
            app_handle,
            "Users",
            &format!("Migrating user: {}", source_user.login_name),
            progress,
        );

        if let Some(&existing_id) = dest_login_map.get(&source_user.login_name.to_lowercase()) {
            let msg = format!(
                "User '{}' already exists on destination (ID: {}) — skipped",
                source_user.login_name, existing_id
            );
            tracing::info!("{}", msg);
            emit_log(app_handle, "debug", &msg);
            summary.skipped.push(msg);
            // Still record access group membership so existing users are included in groups
            for &src_group_id in &source_user.access_group_ids {
                mapping
                    .access_group_members
                    .entry(src_group_id)
                    .or_default()
                    .push(existing_id);
            }
            continue;
        }

        // Determine destination org unit: prefer the user's org_unit_id mapped through
        // mapping.org_units, falling back to the OU we fetched them from
        let dest_customer_id = source_user
            .org_unit_id
            .and_then(|src_org_id| mapping.org_units.get(&src_org_id).cloned())
            .unwrap_or(dest_ou);

        // Roles visible at the user's destination org unit, fetched once per org unit.
        // Role IDs are checked against this list so a role from another org unit is
        // never assigned.
        if let std::collections::hash_map::Entry::Vacant(e) =
            dest_roles_by_ou.entry(dest_customer_id)
        {
            let failures_before = summary.fetch_failures.len();
            let roles = fetch_or_warn(
                dest.get_user_roles(dest_customer_id).await,
                app_handle,
                &format!("dest roles for OU {} (user role mapping)", dest_customer_id),
                &mut summary.fetch_failures,
            );
            let visible = (summary.fetch_failures.len() == failures_before).then(|| {
                roles
                    .into_iter()
                    .filter_map(|role| {
                        role.role_name
                            .map(|name| (name.to_lowercase(), role.role_id))
                    })
                    .collect::<HashMap<String, i64>>()
            });
            e.insert(visible);
        }
        let visible_roles = dest_roles_by_ou
            .get(&dest_customer_id)
            .and_then(Option::as_ref);

        // Map source role IDs -> dest role IDs. Roles that cannot be resolved are
        // recorded so the user is reported for manual follow-up, even when other
        // roles did map.
        let mut mapped_roles: Vec<i64> = Vec::new();
        let mut unmapped_role_labels: Vec<String> = Vec::new();
        for src_role_id in &source_user.role_ids {
            let role_name = source_role_id_to_name.get(src_role_id).map(String::as_str);
            match resolve_destination_role(
                *src_role_id,
                role_name,
                &mapping.roles,
                &mapping.role_names,
                visible_roles,
            ) {
                Some(dest_role_id) => {
                    emit_log(
                        app_handle,
                        "debug",
                        &format!(
                            "Mapped role: src {} ('{}') → dest {}",
                            src_role_id,
                            role_name.unwrap_or("?"),
                            dest_role_id
                        ),
                    );
                    mapped_roles.push(dest_role_id);
                }
                None => {
                    emit_log(
                        app_handle,
                        "debug",
                        &format!(
                            "Could not map source role ID {} to destination for user '{}'",
                            src_role_id, source_user.login_name
                        ),
                    );
                    unmapped_role_labels.push(match role_name {
                        Some(name) => format!("{} (ID {})", name, src_role_id),
                        None => format!("ID {}", src_role_id),
                    });
                }
            }
        }

        emit_log(
            app_handle,
            "debug",
            &format!(
                "User '{}' — source roles: {:?}, mapped dest roles: {:?}, dest OU: {} (org_unit_id: {:?})",
                source_user.login_name, source_user.role_ids, mapped_roles, dest_customer_id, source_user.org_unit_id
            ),
        );
        tracing::info!(
            "User '{}' - source role_ids: {:?}, mapped dest roles: {:?}, dest OU: {}",
            source_user.login_name,
            source_user.role_ids,
            mapped_roles,
            dest_customer_id
        );

        // Use SOAP API for user creation (REST API doesn't support it)
        if let Some(soap) = soap_client {
            emit_log(
                app_handle,
                "info",
                &format!(
                    "Users: Creating user '{}' via SOAP (customerID: {})...",
                    source_user.login_name, dest_customer_id
                ),
            );

            let extra = source_user.extra.as_ref();
            let user_info = UserAddInfo {
                email: source_user
                    .email
                    .clone()
                    .unwrap_or_else(|| source_user.login_name.clone()),
                first_name: source_user.first_name.clone().unwrap_or_default(),
                last_name: source_user.last_name.clone().unwrap_or_default(),
                phone: extra.and_then(|e| e.phone.clone()),
                department: extra.and_then(|e| e.department.clone()),
                location: extra.and_then(|e| e.location.clone()),
                is_enabled: source_user.is_enabled,
                customer_id: dest_customer_id,
                role_ids: mapped_roles.clone(),
                access_group_ids: vec![],
            };

            match soap
                .user_add(&source_user.login_name, &user_info, None)
                .await
            {
                Ok(id) => {
                    if id > 0 {
                        mapping
                            .user_logins
                            .insert(source_user.login_name.to_lowercase(), id);
                        // Record source access group membership for this user
                        for &src_group_id in &source_user.access_group_ids {
                            mapping
                                .access_group_members
                                .entry(src_group_id)
                                .or_default()
                                .push(id);
                        }
                        if !unmapped_role_labels.is_empty() {
                            summary.users_no_roles.push((
                                source_user.login_name.clone(),
                                unmapped_role_labels.clone(),
                            ));
                        }
                        let msg = format!(
                            "Created user '{}' at OU {} (ID: {})",
                            source_user.login_name, dest_customer_id, id
                        );
                        tracing::info!("{}", msg);
                        emit_log(app_handle, "success", &msg);
                    } else {
                        summary.users_failed.push(format!(
                            "{}: the server refused to create the user (returned ID {}); a login that already exists elsewhere on the server cannot be created again",
                            source_user.login_name, id
                        ));
                        let msg = format!(
                            "User '{}' returned ID {} - may already exist elsewhere",
                            source_user.login_name, id
                        );
                        tracing::warn!("{}", msg);
                        emit_log(app_handle, "warning", &msg);
                    }
                }
                Err(e) => {
                    summary.users_failed.push(format!(
                        "{}: SOAP userAdd failed: {}",
                        source_user.login_name, e
                    ));
                    let msg = format!(
                        "Failed to create user '{}' via SOAP: {}",
                        source_user.login_name, e
                    );
                    tracing::error!("{}", msg);
                    emit_log(app_handle, "error", &msg);
                }
            }
        } else {
            summary.users_failed.push(format!(
                "{}: user creation needs the SOAP API, which is not available",
                source_user.login_name
            ));
            let msg = format!(
                "SOAP client not initialized - cannot create user '{}'. Manual creation required.",
                source_user.login_name
            );
            tracing::warn!("{}", msg);
            emit_log(app_handle, "error", &msg);
        }
    }

    Ok(())
}
