//! Access groups, with their org-unit scope and members.

use super::env::{emit_log, fetch_or_warn, report_progress, MigrationEnv};
use super::mapping::map_group_scope;
use super::summary::MigrationSummary;
use super::types::IdMapping;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::Runtime;

/// Migrate access groups from source to destination across all org unit levels.
/// Populates `mapping.access_groups`.
pub(super) async fn migrate_access_groups<R: Runtime>(
    env: &MigrationEnv<'_, R>,
    mapping: &mut IdMapping,
    summary: &mut MigrationSummary,
) -> Result<(), String> {
    let MigrationEnv {
        source,
        dest,
        source_so_id,
        dest_so_id,
        soap: soap_client,
        app_handle,
        cancel_token,
        ..
    } = *env;
    if cancel_token.load(Ordering::Relaxed) {
        return Ok(());
    }
    // Collect all dest org unit IDs (customers + sites) for SO-level access group scope
    let all_dest_ou_ids: Vec<String> = mapping
        .org_units
        .values()
        .filter(|&&v| v != dest_so_id)
        .map(|v| v.to_string())
        .collect();

    // Build ordered org unit pairs: SO first, then customers, then sites.
    // This ensures parent-level groups are processed before children, so inherited
    // groups (which N-central returns at every child OU) are recognized as duplicates.
    let mut org_unit_pairs: Vec<(i64, i64)> = Vec::new();
    if let Some(&dest_so) = mapping.org_units.get(&source_so_id) {
        org_unit_pairs.push((source_so_id, dest_so));
    }
    for (&src, &dst) in &mapping.customers {
        org_unit_pairs.push((src, dst));
    }
    for (&src, &dst) in &mapping.sites {
        org_unit_pairs.push((src, dst));
    }
    let total_ous = org_unit_pairs.len();

    emit_log(
        app_handle,
        "info",
        &format!(
            "Access Groups: Scanning {} org units for access groups...",
            total_ous
        ),
    );

    // Same as roles: a group is listed at every org unit beneath the one that defines it.
    // Visit each source group once, and leave out groups defined outside the source service
    // org (inherited from above it), which have nothing to map to on the destination.
    let mut seen_source_groups: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for (idx, &(src_ou, dest_ou)) in org_unit_pairs.iter().enumerate() {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        let progress = 50.0 + (idx as f32 / total_ous as f32) * 15.0;
        let ou_label = if src_ou == source_so_id {
            "Service Org".to_string()
        } else if mapping.customers.contains_key(&src_ou) {
            format!("Customer {}", src_ou)
        } else {
            format!("Site {}", src_ou)
        };

        report_progress(
            app_handle,
            "Access Groups",
            &format!("Fetching access groups from {} ...", ou_label),
            progress,
        );

        let source_groups = fetch_or_warn(
            source.get_access_groups(src_ou).await,
            app_handle,
            &format!("source access groups at {}", ou_label),
            &mut summary.fetch_failures,
        );
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        if source_groups.is_empty() {
            continue;
        }

        let fetch_failures_before_dest_groups = summary.fetch_failures.len();
        let dest_groups = fetch_or_warn(
            dest.get_access_groups(dest_ou).await,
            app_handle,
            &format!("dest access groups at {}", ou_label),
            &mut summary.fetch_failures,
        );
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        if summary.fetch_failures.len() != fetch_failures_before_dest_groups {
            continue;
        }
        let mut dest_name_map: HashMap<String, i64> = HashMap::new();
        for g in &dest_groups {
            if let Some(ref name) = g.group_name {
                dest_name_map.insert(name.to_lowercase(), g.group_id);
            }
        }

        // Determine orgUnitIds scope: SO-level groups get all customers/sites,
        // others get just their own OU
        let scope_ou_ids = if src_ou == source_so_id {
            all_dest_ou_ids.clone()
        } else {
            vec![dest_ou.to_string()]
        };

        for source_group in source_groups {
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }
            if !seen_source_groups.insert(source_group.group_id) {
                continue;
            }
            let group_name = source_group
                .group_name
                .as_deref()
                .unwrap_or("Unknown Group");
            if source_group
                .org_unit_id
                .is_some_and(|defined_at| !mapping.org_units.contains_key(&defined_at))
            {
                let msg = format!(
                    "Access group '{}' is defined outside the source service org (org unit {}) and was not migrated",
                    group_name,
                    source_group.org_unit_id.unwrap_or_default()
                );
                tracing::info!("{}", msg);
                emit_log(app_handle, "debug", &msg);
                summary.skipped.push(msg);
                continue;
            }
            let group_name_lower = group_name.to_lowercase();

            // Use source user→access group membership data to determine which dest
            // users should be in this group (replicating the source exactly).
            let member_user_ids: Vec<String> = mapping
                .access_group_members
                .get(&source_group.group_id)
                .map(|ids| ids.iter().map(|id| id.to_string()).collect())
                .unwrap_or_default();

            // Check if group already exists at this dest OU (including inherited groups).
            // Like roles, we use dest_name_map per-OU rather than global dedup,
            // because same-named groups at sibling OUs are distinct.
            if let Some(&id) = dest_name_map.get(&group_name_lower) {
                mapping.access_groups.insert(source_group.group_id, id);
                // N-central cannot change the membership of an existing group, so it only
                // needs follow-up if a migrated user is missing from it.
                let expected_members: Vec<i64> = mapping
                    .access_group_members
                    .get(&source_group.group_id)
                    .cloned()
                    .unwrap_or_default();
                let missing_members = if expected_members.is_empty() {
                    0
                } else {
                    match dest.get_access_group_details(id).await {
                        Ok(details) => {
                            let current = details.user_ids.unwrap_or_default();
                            expected_members
                                .iter()
                                .filter(|user_id| !current.contains(user_id))
                                .count()
                        }
                        // Cannot verify, so assume the migrated users are missing.
                        Err(_) => expected_members.len(),
                    }
                };
                if missing_members > 0 {
                    summary.access_groups_existed_not_updated.push(format!(
                        "'{}' (at {}) is missing {} migrated user(s)",
                        group_name, ou_label, missing_members
                    ));
                }
                let msg = format!(
                    "Access group '{}' already exists at dest {} (ID: {})",
                    group_name, ou_label, id
                );
                tracing::debug!("{}", msg);
                emit_log(app_handle, "debug", &msg);
                summary.skipped.push(msg);
            } else {
                let group_type = source_group
                    .get_group_type()
                    .unwrap_or_else(|| "ORG_UNIT".into());
                if group_type.eq_ignore_ascii_case("DEVICE") {
                    let msg = format!(
                        "Device access group '{}' at {} needs manual follow-up because device ID mapping is not implemented",
                        group_name, ou_label
                    );
                    tracing::warn!("{}", msg);
                    emit_log(app_handle, "warning", &msg);
                    summary.additional_manual_follow_up.push(msg);
                    continue;
                }
                if !group_type.eq_ignore_ascii_case("ORG_UNIT") {
                    let msg = format!(
                        "Access group '{}' at {} has unsupported type '{}' and needs manual follow-up",
                        group_name, ou_label, group_type
                    );
                    tracing::warn!("{}", msg);
                    emit_log(app_handle, "warning", &msg);
                    summary.additional_manual_follow_up.push(msg);
                    continue;
                }
                let description = source_group.group_description.as_deref().unwrap_or("");

                // The per-org-unit group list usually omits the org units a group covers and
                // its auto-include setting; the group's detail endpoint has both. Read it when
                // the list did not say, and fall back to the computed scope only if that fails.
                let mut source_ou_ids = source_group.get_org_unit_ids();
                let mut source_auto_include = source_group.get_auto_include();
                if source_ou_ids.is_empty() {
                    match source.get_access_group_details(source_group.group_id).await {
                        Ok(details) => {
                            source_ou_ids = details.get_org_unit_ids();
                            source_auto_include =
                                source_auto_include.or(details.get_auto_include());
                        }
                        Err(e) => {
                            let msg = format!(
                                "Access group '{}' at {}: could not read its org-unit scope ({}); it was created with the default scope for its level",
                                group_name, ou_label, e
                            );
                            tracing::warn!("{}", msg);
                            emit_log(app_handle, "warning", &msg);
                            summary.additional_manual_follow_up.push(msg);
                        }
                    }
                }
                let (effective_ou_ids, unmapped_scope): (Vec<String>, Vec<i64>) =
                    if !source_ou_ids.is_empty() {
                        // Map source org unit IDs to destination IDs
                        map_group_scope(&source_ou_ids, &mapping.org_units)
                    } else {
                        (scope_ou_ids.clone(), Vec::new())
                    };
                if effective_ou_ids.is_empty() {
                    let msg = format!(
                        "Access group '{}' at {} was not created: none of its {} org unit(s) exist on the destination",
                        group_name,
                        ou_label,
                        source_ou_ids.len()
                    );
                    tracing::warn!("{}", msg);
                    emit_log(app_handle, "warning", &msg);
                    summary
                        .access_groups_failed
                        .push(format!("{} (at {})", group_name, ou_label));
                    summary.additional_manual_follow_up.push(msg);
                    continue;
                }
                let narrowed_scope_note = (!unmapped_scope.is_empty()).then(|| {
                    format!(
                        "Access group '{}' at {} was created for {} of {} org unit(s); no destination match for source org unit(s) {}",
                        group_name,
                        ou_label,
                        effective_ou_ids.len(),
                        source_ou_ids.len(),
                        unmapped_scope
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                });

                let auto_include = source_auto_include.unwrap_or(true).to_string();

                let payload = serde_json::json!({
                    "groupName": group_name,
                    "groupDescription": description,
                    "orgUnitIds": effective_ou_ids,
                    "userIds": member_user_ids,
                    "autoIncludeNewOrgUnits": auto_include
                });

                tracing::info!(
                    "Creating access group '{}' at {} (type: {}) with {} org units and {} users...",
                    group_name,
                    ou_label,
                    group_type,
                    effective_ou_ids.len(),
                    member_user_ids.len()
                );

                let result = dest.create_org_unit_access_group(dest_ou, &payload).await;

                match result {
                    Ok(resp) => {
                        let data = if resp.get("data").is_some() {
                            &resp["data"]
                        } else {
                            &resp
                        };
                        let mut id = data["groupId"]
                            .as_i64()
                            .or_else(|| data["accessGroupId"].as_i64())
                            .or_else(|| data["id"].as_i64())
                            .or_else(|| resp["groupId"].as_i64())
                            .or_else(|| resp["id"].as_i64())
                            .unwrap_or(0);
                        if id <= 0 {
                            // N-central answers a successful group creation with no body, so
                            // the new group's ID has to be looked up by name.
                            id = dest
                                .find_access_group_id(dest_ou, group_name)
                                .await
                                .ok()
                                .flatten()
                                .unwrap_or(0);
                        }
                        if id > 0 {
                            mapping.access_groups.insert(source_group.group_id, id);
                            summary
                                .access_groups_created
                                .push(format!("{} (at {})", group_name, ou_label));
                            if let Some(note) = narrowed_scope_note.clone() {
                                emit_log(app_handle, "warning", &note);
                                summary.additional_manual_follow_up.push(note);
                            }
                            let msg = format!(
                                "Created access group '{}' at {} (ID: {})",
                                group_name, ou_label, id
                            );
                            tracing::info!("{}", msg);
                            emit_log(app_handle, "success", &msg);
                        } else {
                            summary
                                .access_groups_failed
                                .push(format!("{} (at {})", group_name, ou_label));
                            let msg = format!(
                                "Access group '{}' at {} created but no ID returned in response: {:?}",
                                group_name, ou_label, resp
                            );
                            tracing::warn!("{}", msg);
                            emit_log(app_handle, "warning", &msg);
                        }
                    }
                    Err(rest_err) => {
                        if cancel_token.load(Ordering::Relaxed) {
                            break;
                        }
                        tracing::warn!(
                            "REST create access group failed for '{}': {}. Trying SOAP...",
                            group_name,
                            rest_err
                        );
                        if let Some(soap) = soap_client {
                            match soap
                                .access_group_add(
                                    group_name,
                                    description,
                                    dest_ou,
                                    &group_type,
                                    auto_include == "true",
                                )
                                .await
                            {
                                Ok(id) => {
                                    if id > 0 {
                                        mapping.access_groups.insert(source_group.group_id, id);
                                        // SOAP accessGroupAdd takes neither members nor an
                                        // org-unit list, so this group needs manual completion.
                                        summary
                                            .access_groups_created_via_soap
                                            .push(format!("{} (at {})", group_name, ou_label));
                                        let msg = format!(
                                            "Created access group '{}' at {} via SOAP (ID: {})",
                                            group_name, ou_label, id
                                        );
                                        tracing::info!("{}", msg);
                                        emit_log(app_handle, "success", &msg);
                                    } else {
                                        summary
                                            .access_groups_failed
                                            .push(format!("{} (at {})", group_name, ou_label));
                                        let msg = format!(
                                            "SOAP access_group_add '{}' at {} returned non-positive ID {}",
                                            group_name, ou_label, id
                                        );
                                        tracing::warn!("{}", msg);
                                        emit_log(app_handle, "warning", &msg);
                                    }
                                }
                                Err(soap_err) => {
                                    summary
                                        .access_groups_failed
                                        .push(format!("{} (at {})", group_name, ou_label));
                                    let msg = format!(
                                        "Failed to create access group '{}' at {} (REST: {}, SOAP: {})",
                                        group_name, ou_label, rest_err, soap_err
                                    );
                                    tracing::error!("{}", msg);
                                    emit_log(app_handle, "error", &msg);
                                }
                            }
                        } else {
                            summary
                                .access_groups_failed
                                .push(format!("{} (at {})", group_name, ou_label));
                            let msg = format!(
                                "Failed to create access group '{}' at {}: {}",
                                group_name, ou_label, rest_err
                            );
                            tracing::error!("{}", msg);
                            emit_log(app_handle, "error", &msg);
                        }
                    }
                }
            }
        }
    }

    emit_log(
        app_handle,
        "info",
        &format!(
            "Access Groups: {} total mappings across {} org units",
            mapping.access_groups.len(),
            total_ous
        ),
    );

    Ok(())
}
