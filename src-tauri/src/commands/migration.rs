//! Migration-related Tauri commands

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, State};

use crate::api::client::NcClient;
use crate::api::{NcSoapClient, UserAddInfo};
use crate::commands::connection::{AppState, ConnectionResult};
use crate::models::*;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MigrationOptions {
    pub customers: bool,
    pub user_roles: bool,
    pub access_groups: bool,
    pub users: bool,
    pub org_properties: bool,
}

/// Mapping of Source IDs to Destination IDs and name-based lookups
pub struct IdMapping {
    /// Source Customer ID -> Destination Customer ID
    pub customers: HashMap<i64, i64>,
    /// Source Site ID -> Destination Site ID
    pub sites: HashMap<i64, i64>,
    /// Source Role ID -> Destination Role ID
    pub roles: HashMap<i64, i64>,
    /// Source Access Group ID -> Destination Access Group ID
    pub access_groups: HashMap<i64, i64>,
    /// Role Name (lowercase) -> Destination Role ID (fallback for user creation)
    pub role_names: HashMap<String, i64>,
    /// User Login Name (lowercase) -> Destination User ID
    pub user_logins: HashMap<String, i64>,
    /// Source Org Unit ID -> Destination Org Unit ID (SO, Customer, or Site)
    pub org_units: HashMap<i64, i64>,
    /// Source Access Group ID -> list of Destination User IDs that should be members.
    /// Built during user migration from each source user's access_group_ids field.
    pub access_group_members: HashMap<i64, Vec<i64>>,
}

impl IdMapping {
    pub fn new() -> Self {
        Self {
            customers: HashMap::new(),
            sites: HashMap::new(),
            roles: HashMap::new(),
            access_groups: HashMap::new(),
            role_names: HashMap::new(),
            user_logins: HashMap::new(),
            org_units: HashMap::new(),
            access_group_members: HashMap::new(),
        }
    }
}

/// Helper to emit progress updates
fn report_progress(app_handle: &AppHandle, phase: &str, message: &str, percent: f32) {
    let _ = app_handle.emit(
        "export-progress",
        ProgressUpdate {
            phase: phase.to_string(),
            message: message.to_string(),
            percent,
            current: 0,
            total: 100,
        },
    );
}

/// Helper to emit log messages to the frontend Activity Log
fn emit_log(app_handle: &AppHandle, level: &str, message: &str) {
    let _ = app_handle.emit(
        "backend-log",
        LogMessage {
            level: level.to_string(),
            message: message.to_string(),
        },
    );
}

// ==================== Entity Migration Functions ====================

/// Migrate customers and sites from source to destination.
/// Populates `mapping.customers`, `mapping.sites`, and `mapping.org_units`.
async fn migrate_customers_and_sites(
    source: &NcClient,
    dest: &NcClient,
    source_so_id: i64,
    dest_so_id: i64,
    mapping: &mut IdMapping,
    soap_client: Option<&NcSoapClient>,
    app_handle: &AppHandle,
) -> Result<(), String> {
    report_progress(app_handle, "Customers", "Fetching source customers...", 10.0);
    let source_customers = source
        .get_customers_by_so(source_so_id)
        .await
        .map_err(|e| format!("Failed to fetch source customers: {}", e))?;

    report_progress(app_handle, "Customers", "Fetching destination customers...", 15.0);
    let dest_customers = dest
        .get_customers_by_so(dest_so_id)
        .await
        .map_err(|e| format!("Failed to fetch destination customers: {}", e))?;

    let mut dest_name_map_raw = HashMap::new();
    for c in &dest_customers {
        dest_name_map_raw.insert(c.customer_name.to_lowercase(), c.customer_id);
    }
    let dest_name_map = std::sync::Arc::new(dest_name_map_raw);

    // Build source customer id -> name map for later site mapping
    let source_cust_id_to_name: HashMap<i64, String> = source_customers
        .iter()
        .map(|c| (c.customer_id, c.customer_name.clone()))
        .collect();

    let total = source_customers.len();

    use futures::stream::{self, StreamExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let completed = Arc::new(AtomicUsize::new(0));

    let bodies = stream::iter(source_customers)
        .map(|source_cust| {
            let dest = dest.clone();
            let dest_name_map = dest_name_map.clone();
            let completed = completed.clone();
            let app_handle = app_handle.clone();

            async move {
                let dest_id = if let Some(&id) =
                    dest_name_map.get(&source_cust.customer_name.to_lowercase())
                {
                    let msg = format!(
                        "Customer '{}' already exists on destination (ID: {})",
                        source_cust.customer_name, id
                    );
                    tracing::info!("{}", msg);
                    emit_log(&app_handle, "debug", &msg);
                    id
                } else {
                    let payload = serde_json::json!({
                        "customerName": source_cust.customer_name,
                        "parentId": dest_so_id,
                        "externalId": source_cust.external_id,
                        "contactFirstName": source_cust.contact_first_name,
                        "contactLastName": source_cust.contact_last_name,
                        "contactEmail": source_cust.contact_email,
                    });

                    match dest.create_customer(dest_so_id, &payload).await {
                        Ok(resp) => {
                            let data = if resp.get("data").is_some() { &resp["data"] } else { &resp };
                            let id = data["customerId"]
                                .as_i64()
                                .or_else(|| data["id"].as_i64())
                                .or_else(|| resp["customerId"].as_i64())
                                .or_else(|| resp["id"].as_i64())
                                .unwrap_or(0);
                            let msg = format!(
                                "Created customer '{}' (ID: {})",
                                source_cust.customer_name, id
                            );
                            tracing::info!("{}", msg);
                            emit_log(&app_handle, "success", &msg);
                            id
                        }
                        Err(rest_err) => {
                            // REST failed — try SOAP fallback
                            tracing::warn!("REST create_customer failed for '{}': {}. Trying SOAP fallback...", source_cust.customer_name, rest_err);
                            if let Some(soap) = soap_client {
                                match soap.customer_add(
                                    &source_cust.customer_name,
                                    dest_so_id,
                                    source_cust.external_id.as_deref(),
                                    source_cust.contact_first_name.as_deref(),
                                    source_cust.contact_last_name.as_deref(),
                                    source_cust.contact_email.as_deref(),
                                ).await {
                                    Ok(id) => {
                                        let msg = format!("Created customer '{}' via SOAP (ID: {})", source_cust.customer_name, id);
                                        tracing::info!("{}", msg);
                                        emit_log(&app_handle, "success", &msg);
                                        id
                                    }
                                    Err(soap_err) => {
                                        let msg = format!("Failed to create customer '{}' (REST: {}, SOAP: {})", source_cust.customer_name, rest_err, soap_err);
                                        tracing::error!("{}", msg);
                                        emit_log(&app_handle, "error", &msg);
                                        0
                                    }
                                }
                            } else {
                                let msg = format!("Failed to create customer '{}': {}", source_cust.customer_name, rest_err);
                                tracing::error!("{}", msg);
                                emit_log(&app_handle, "error", &msg);
                                0
                            }
                        }
                    }
                };

                let count = completed.fetch_add(1, Ordering::SeqCst) + 1;
                let progress = 15.0 + (count as f32 / total as f32) * 15.0;
                let _ = app_handle.emit(
                    "export-progress",
                    ProgressUpdate {
                        phase: "Customers".to_string(),
                        message: format!("Processed customer: {}", source_cust.customer_name),
                        percent: progress,
                        current: count as u32,
                        total: total as u32,
                    },
                );

                (source_cust.customer_id, dest_id)
            }
        })
        .buffer_unordered(2);

    let results: Vec<(i64, i64)> = bodies.collect().await;

    for (source_id, dest_id) in results {
        if dest_id != 0 {
            mapping.customers.insert(source_id, dest_id);
            mapping.org_units.insert(source_id, dest_id);
        }
    }

    // Map the Service Org IDs as well
    mapping.org_units.insert(source_so_id, dest_so_id);

    // --- Sites ---
    report_progress(app_handle, "Sites", "Fetching source sites...", 30.0);
    let source_sites = source
        .get_sites_by_so(source_so_id)
        .await
        .unwrap_or_default();
    let dest_sites = dest.get_sites_by_so(dest_so_id).await.unwrap_or_default();

    // Build dest site lookup: (parent_customer_name, site_name) -> site_id
    let mut dest_site_lookup: HashMap<(String, String), i64> = HashMap::new();
    for site in &dest_sites {
        let parent_id = site.parent_id.or(site.customer_id).or(site.customerid);
        if let Some(pid) = parent_id {
            if let Some(dest_cust) = dest_customers.iter().find(|c| c.customer_id == pid) {
                dest_site_lookup.insert(
                    (
                        dest_cust.customer_name.to_lowercase(),
                        site.site_name.to_lowercase(),
                    ),
                    site.site_id,
                );
            }
        }
    }

    for source_site in source_sites {
        let source_parent_id = source_site
            .parent_id
            .or(source_site.customer_id)
            .or(source_site.customerid);
        if let Some(src_parent_id) = source_parent_id {
            if let Some(src_cust_name) = source_cust_id_to_name.get(&src_parent_id) {
                let key = (
                    src_cust_name.to_lowercase(),
                    source_site.site_name.to_lowercase(),
                );
                if let Some(&dest_site_id) = dest_site_lookup.get(&key) {
                    mapping.sites.insert(source_site.site_id, dest_site_id);
                    mapping.org_units.insert(source_site.site_id, dest_site_id);
                    let msg = format!(
                        "Site '{}' under '{}' already exists on destination (ID: {})",
                        source_site.site_name, src_cust_name, dest_site_id
                    );
                    tracing::debug!("{}", msg);
                    emit_log(app_handle, "debug", &msg);
                } else if let Some(&dest_cust_id) = mapping.customers.get(&src_parent_id) {
                    let msg = format!(
                        "Creating site '{}' under customer '{}' (dest customer ID: {})...",
                        source_site.site_name, src_cust_name, dest_cust_id
                    );
                    tracing::info!("{}", msg);
                    emit_log(app_handle, "info", &msg);

                    let payload = serde_json::json!({
                        "siteName": source_site.site_name,
                        "externalId": source_site.external_id,
                        "contactFirstName": source_site.contact_first_name,
                        "contactLastName": source_site.contact_last_name,
                        "contactEmail": source_site.contact_email,
                        "contactPhone": source_site.contact_phone,
                        "street1": source_site.street1,
                        "street2": source_site.street2,
                        "city": source_site.city,
                        "stateProv": source_site.state_prov,
                        "country": source_site.country,
                        "postalCode": source_site.postal_code,
                    });

                    match dest.create_site(dest_cust_id, &payload).await {
                        Ok(resp) => {
                            let data = if resp.get("data").is_some() { &resp["data"] } else { &resp };
                            let id = data["siteId"]
                                .as_i64()
                                .or_else(|| data["id"].as_i64())
                                .or_else(|| resp["siteId"].as_i64())
                                .or_else(|| resp["id"].as_i64())
                                .unwrap_or(0);
                            if id != 0 {
                                mapping.sites.insert(source_site.site_id, id);
                                mapping.org_units.insert(source_site.site_id, id);
                                let msg = format!(
                                    "Created site '{}' (ID: {})",
                                    source_site.site_name, id
                                );
                                tracing::info!("{}", msg);
                                emit_log(app_handle, "success", &msg);
                            } else {
                                let msg = format!("Site '{}' created but no ID returned in response: {:?}", source_site.site_name, resp);
                                tracing::warn!("{}", msg);
                                emit_log(app_handle, "warning", &msg);
                            }
                        }
                        Err(e) => {
                            let msg = format!(
                                "Failed to create site '{}': {}",
                                source_site.site_name, e
                            );
                            tracing::error!("{}", msg);
                            emit_log(app_handle, "error", &msg);
                        }
                    }
                } else {
                    let msg = format!("Site '{}' under customer '{}' skipped - parent customer not mapped to destination", source_site.site_name, src_cust_name);
                    tracing::warn!("{}", msg);
                    emit_log(app_handle, "warning", &msg);
                }
            }
        }
    }

    let msg = format!(
        "Org unit mapping: {} customers, {} sites, {} total org_units",
        mapping.customers.len(),
        mapping.sites.len(),
        mapping.org_units.len()
    );
    tracing::info!("{}", msg);
    emit_log(app_handle, "info", &msg);

    Ok(())
}

/// Migrate user roles from source to destination across all org unit levels.
/// Populates `mapping.roles` and `mapping.role_names`.
async fn migrate_user_roles(
    source: &NcClient,
    dest: &NcClient,
    source_so_id: i64,
    _dest_so_id: i64,
    mapping: &mut IdMapping,
    soap_client: Option<&NcSoapClient>,
    app_handle: &AppHandle,
) -> Result<(), String> {
    report_progress(app_handle, "Roles", "Loading permission mappings...", 28.0);

    let perm_csv = include_str!("../../rolePermissionIds.csv");
    let perm_lookup = crate::models::user_role::PermissionLookup::from_csv(perm_csv);

    if perm_lookup.is_empty() {
        tracing::warn!("No permission mappings loaded from CSV - roles will be created with minimal permissions");
        emit_log(app_handle, "warning", "No permission mappings loaded — roles will have minimal permissions");
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

    for (idx, &(src_ou, dest_ou)) in org_unit_pairs.iter().enumerate() {
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

        let source_roles = source.get_user_roles(src_ou).await.unwrap_or_default();
        if source_roles.is_empty() {
            continue;
        }

        let dest_roles = dest.get_user_roles(dest_ou).await.unwrap_or_default();
        let mut dest_name_map: HashMap<String, i64> = HashMap::new();
        for r in &dest_roles {
            if let Some(ref name) = r.role_name {
                dest_name_map.insert(name.to_lowercase(), r.role_id);
            }
        }

        for source_role in source_roles {
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
            } else {
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

                let payload = serde_json::json!({
                    "roleName": role_name,
                    "description": description,
                    "permissionIds": permission_ids,
                    "userIds": []
                });

                tracing::info!(
                    "Creating role '{}' at {} with {} permissions...",
                    role_name, ou_label, permission_ids.len()
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
                        match soap.user_role_add(role_name, description, dest_ou, &permission_ids).await {
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

                if !created {
                    // REST fallback (works well for SO-level roles; for others, re-fetch picks up the ID)
                    match dest.create_user_role(dest_ou, &payload).await {
                        Ok(resp) => {
                            let data = if resp.get("data").is_some() { &resp["data"] } else { &resp };
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
                                    role_name, ou_label, id, permission_ids.len()
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
                            emit_log(app_handle, "debug", &format!(
                                "REST create_user_role '{}' at {} failed: {}",
                                role_name, ou_label, rest_err
                            ));
                        }
                    }
                }
            }
        }

        // Safety net: re-fetch dest roles to pick up any that were created but whose
        // IDs weren't captured from the create response. Match source roles by name.
        let refreshed_dest_roles = dest.get_user_roles(dest_ou).await.unwrap_or_default();
        let mut refreshed_name_map: HashMap<String, i64> = HashMap::new();
        for r in &refreshed_dest_roles {
            if let Some(ref name) = r.role_name {
                refreshed_name_map.insert(name.to_lowercase(), r.role_id);
            }
        }

        // Re-fetch source roles to fill in any mapping gaps
        let source_roles_refetch = source.get_user_roles(src_ou).await.unwrap_or_default();
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

/// Migrate access groups from source to destination across all org unit levels.
/// Populates `mapping.access_groups`.
async fn migrate_access_groups(
    source: &NcClient,
    dest: &NcClient,
    source_so_id: i64,
    dest_so_id: i64,
    mapping: &mut IdMapping,
    soap_client: Option<&NcSoapClient>,
    app_handle: &AppHandle,
) -> Result<(), String> {
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

    for (idx, &(src_ou, dest_ou)) in org_unit_pairs.iter().enumerate() {
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

        let source_groups = source.get_access_groups(src_ou).await.unwrap_or_default();
        if source_groups.is_empty() {
            continue;
        }

        let dest_groups = dest.get_access_groups(dest_ou).await.unwrap_or_default();
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
            let group_name = source_group
                .group_name
                .as_deref()
                .unwrap_or("Unknown Group");
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
                let msg = format!(
                    "Access group '{}' already exists at dest {} (ID: {})",
                    group_name, ou_label, id
                );
                tracing::debug!("{}", msg);
                emit_log(app_handle, "debug", &msg);
            } else {
                let group_type = source_group.group_type.as_deref().unwrap_or("ORG_UNIT");
                let description = source_group.group_description.as_deref().unwrap_or("");

                // Use source _extra data for orgUnitIds and autoInclude when available,
                // otherwise fall back to computed scope
                let source_ou_ids = source_group.get_org_unit_ids();
                let effective_ou_ids: Vec<String> = if !source_ou_ids.is_empty() {
                    // Map source org unit IDs to destination IDs
                    source_ou_ids
                        .iter()
                        .filter_map(|&src_id| mapping.org_units.get(&src_id).map(|d| d.to_string()))
                        .collect()
                } else {
                    scope_ou_ids.clone()
                };

                let auto_include = source_group
                    .get_auto_include()
                    .unwrap_or_else(|| "true".to_string());

                let payload = serde_json::json!({
                    "groupName": group_name,
                    "groupDescription": description,
                    "orgUnitIds": effective_ou_ids,
                    "userIds": member_user_ids,
                    "autoIncludeNewOrgUnits": auto_include
                });

                tracing::info!(
                    "Creating access group '{}' at {} (type: {}) with {} org units and {} users...",
                    group_name, ou_label, group_type, effective_ou_ids.len(), member_user_ids.len()
                );

                let result = if group_type == "DEVICE" {
                    dest.create_device_access_group(dest_ou, &payload).await
                } else {
                    dest.create_org_unit_access_group(dest_ou, &payload).await
                };

                match result {
                    Ok(resp) => {
                        let data = if resp.get("data").is_some() { &resp["data"] } else { &resp };
                        let id = data["groupId"]
                            .as_i64()
                            .or_else(|| data["accessGroupId"].as_i64())
                            .or_else(|| data["id"].as_i64())
                            .or_else(|| resp["groupId"].as_i64())
                            .or_else(|| resp["id"].as_i64())
                            .unwrap_or(0);
                        if id > 0 {
                            mapping.access_groups.insert(source_group.group_id, id);
                            let msg = format!(
                                "Created access group '{}' at {} (ID: {})",
                                group_name, ou_label, id
                            );
                            tracing::info!("{}", msg);
                            emit_log(app_handle, "success", &msg);
                        }
                    }
                    Err(rest_err) => {
                        tracing::warn!(
                            "REST create access group failed for '{}': {}. Trying SOAP...",
                            group_name, rest_err
                        );
                        if let Some(soap) = soap_client {
                            match soap
                                .access_group_add(
                                    group_name,
                                    description,
                                    dest_ou,
                                    group_type,
                                    true,
                                )
                                .await
                            {
                                Ok(id) => {
                                    if id > 0 {
                                        mapping.access_groups.insert(source_group.group_id, id);
                                                    let msg = format!(
                                            "Created access group '{}' at {} via SOAP (ID: {})",
                                            group_name, ou_label, id
                                        );
                                        tracing::info!("{}", msg);
                                        emit_log(app_handle, "success", &msg);
                                    }
                                }
                                Err(soap_err) => {
                                    let msg = format!(
                                        "Failed to create access group '{}' at {} (REST: {}, SOAP: {})",
                                        group_name, ou_label, rest_err, soap_err
                                    );
                                    tracing::error!("{}", msg);
                                    emit_log(app_handle, "error", &msg);
                                }
                            }
                        } else {
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

/// Migrate users from source to destination via SOAP API across all org unit levels.
/// Fetches users from each org unit (SO, customers, sites) and creates them at the
/// matching destination org unit. Populates `mapping.user_logins`.
async fn migrate_users(
    source: &NcClient,
    dest: &NcClient,
    source_so_id: i64,
    _dest_so_id: i64,
    mapping: &mut IdMapping,
    soap_client: Option<&NcSoapClient>,
    app_handle: &AppHandle,
) -> Result<(), String> {
    // Build a complete source role_id -> role_name map across ALL org units
    report_progress(app_handle, "Users", "Fetching source users and roles...", 70.0);
    let mut source_role_id_to_name: HashMap<i64, String> = HashMap::new();
    for &src_ou in mapping.org_units.keys() {
        let roles = source.get_user_roles(src_ou).await.unwrap_or_default();
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
            let dest_roles = dest.get_user_roles(dest_ou).await.unwrap_or_default();
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
    for &dest_ou in mapping.org_units.values() {
        let dest_users = dest.get_users_by_org_unit(dest_ou).await.unwrap_or_default();
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

        let users = source.get_users_by_org_unit(src_ou).await.unwrap_or_default();
        for user in users {
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
    let total = all_source_users.len();
    for (i, (source_user, dest_ou)) in all_source_users.into_iter().enumerate() {
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
            // Still record access group membership so existing users are included in groups
            for &src_group_id in &source_user.access_group_ids {
                mapping.access_group_members.entry(src_group_id).or_default().push(existing_id);
            }
            continue;
        }

        // Map source role IDs -> dest role IDs via direct ID mapping (populated during role migration).
        // Falls back to name-based lookup if direct mapping is missing.
        let mapped_roles: Vec<i64> = source_user
            .role_ids
            .iter()
            .filter_map(|src_role_id| {
                // Primary: direct source_role_id → dest_role_id mapping
                if let Some(&dest_role_id) = mapping.roles.get(src_role_id) {
                    let role_name = source_role_id_to_name.get(src_role_id).map(|s| s.as_str()).unwrap_or("?");
                    emit_log(
                        app_handle,
                        "debug",
                        &format!("Mapped role: src {} ('{}') → dest {}", src_role_id, role_name, dest_role_id),
                    );
                    return Some(dest_role_id);
                }
                // Fallback: name-based lookup (for roles that weren't in the migration batch)
                if let Some(role_name) = source_role_id_to_name.get(src_role_id) {
                    if let Some(&dest_role_id) = mapping.role_names.get(role_name) {
                        emit_log(
                            app_handle,
                            "debug",
                            &format!("Mapped role via name: src {} ('{}') → dest {}", src_role_id, role_name, dest_role_id),
                        );
                        return Some(dest_role_id);
                    }
                }
                emit_log(
                    app_handle,
                    "debug",
                    &format!(
                        "Could not map source role ID {} to destination for user '{}'",
                        src_role_id, source_user.login_name
                    ),
                );
                None
            })
            .collect();

        // Determine destination org unit: prefer the user's org_unit_id mapped through
        // mapping.org_units, falling back to the OU we fetched them from
        let dest_customer_id = source_user
            .org_unit_id
            .and_then(|src_org_id| mapping.org_units.get(&src_org_id).cloned())
            .unwrap_or(dest_ou);

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
                role_ids: mapped_roles,
                access_group_ids: vec![],
            };

            match soap.user_add(&source_user.login_name, &user_info).await {
                Ok(id) => {
                    if id > 0 {
                        mapping
                            .user_logins
                            .insert(source_user.login_name.to_lowercase(), id);
                        // Record source access group membership for this user
                        for &src_group_id in &source_user.access_group_ids {
                            mapping.access_group_members.entry(src_group_id).or_default().push(id);
                        }
                        let msg = format!(
                            "Created user '{}' at OU {} (ID: {})",
                            source_user.login_name, dest_customer_id, id
                        );
                        tracing::info!("{}", msg);
                        emit_log(app_handle, "success", &msg);
                    } else {
                        let msg = format!(
                            "User '{}' returned ID {} - may already exist elsewhere",
                            source_user.login_name, id
                        );
                        tracing::warn!("{}", msg);
                        emit_log(app_handle, "warning", &msg);
                    }
                }
                Err(e) => {
                    let msg = format!(
                        "Failed to create user '{}' via SOAP: {}",
                        source_user.login_name, e
                    );
                    tracing::error!("{}", msg);
                    emit_log(app_handle, "error", &msg);
                }
            }
        } else {
            tracing::warn!(
                "SOAP client not initialized - cannot create user '{}'. Manual creation required.",
                source_user.login_name
            );
        }
    }

    Ok(())
}

/// Migrate organization custom properties from source to destination.
async fn migrate_org_properties(
    source: &NcClient,
    dest: &NcClient,
    source_so_id: i64,
    mapping: &IdMapping,
    soap_client: Option<&NcSoapClient>,
    app_handle: &AppHandle,
) -> Result<(), String> {
    report_progress(app_handle, "Properties", "Migrating org custom properties...", 90.0);

    let source_props = source
        .get_org_properties(source_so_id)
        .await
        .map_err(|e| format!("Failed to fetch source properties: {}", e))?;

    if source_props.is_empty() {
        emit_log(app_handle, "info", "No custom properties found to migrate.");
        return Ok(());
    }

    emit_log(
        app_handle,
        "info",
        &format!("Found {} custom properties to sync.", source_props.len()),
    );

    let mut synced = 0;
    let mut skipped = 0;
    let mut failed = 0;

    for prop in &source_props {
        // Determine the destination org unit ID
        let dest_ou_id = match prop.org_unit_id {
            Some(src_ou_id) => {
                if let Some(&mapped_id) = mapping.org_units.get(&src_ou_id) {
                    mapped_id
                } else if src_ou_id == source_so_id {
                    // Property belongs to the source SO root — skip or use a default
                    emit_log(app_handle, "debug", &format!("Property {:?} belongs to source SO root — skipped", prop.label));
                    skipped += 1;
                    continue;
                } else {
                    emit_log(app_handle, "debug", &format!("No mapping for source org unit {} (property: {:?}) — skipped", src_ou_id, prop.label));
                    skipped += 1;
                    continue;
                }
            }
            None => {
                emit_log(app_handle, "debug", &format!("Property {:?} has no org_unit_id — skipped", prop.label));
                skipped += 1;
                continue;
            }
        };

        // Build the value payload for the destination
        let value_payload = serde_json::json!({
            "propertyId": prop.property_id,
            "orgUnitId": dest_ou_id,
            "value": prop.value,
        });

        match dest.set_custom_property_value(&value_payload).await {
            Ok(_) => {
                synced += 1;
                emit_log(app_handle, "debug", &format!("Synced property {:?} to OU {}", prop.label, dest_ou_id));
            }
            Err(rest_err) => {
                // REST failed — try SOAP fallback
                if let Some(soap) = soap_client {
                    let prop_value = prop.value.as_deref().unwrap_or("");
                    match soap.organization_property_modify(dest_ou_id, prop.property_id, prop_value).await {
                        Ok(_) => {
                            synced += 1;
                            emit_log(app_handle, "debug", &format!("Synced property {:?} to OU {} via SOAP fallback", prop.label, dest_ou_id));
                        }
                        Err(soap_err) => {
                            let msg = format!(
                                "Failed to sync property {:?} to OU {} (REST: {}, SOAP: {})",
                                prop.label, dest_ou_id, rest_err, soap_err
                            );
                            tracing::warn!("{}", msg);
                            emit_log(app_handle, "debug", &msg);
                            failed += 1;
                        }
                    }
                } else {
                    let msg = format!(
                        "Failed to sync property {:?} to OU {}: {}",
                        prop.label, dest_ou_id, rest_err
                    );
                    tracing::warn!("{}", msg);
                    emit_log(app_handle, "debug", &msg);
                    failed += 1;
                }
            }
        }
    }

    emit_log(
        app_handle,
        "info",
        &format!(
            "Properties: {} synced, {} skipped, {} failed",
            synced, skipped, failed
        ),
    );

    Ok(())
}

/// Collects per-category outcomes for the post-migration summary report.
#[derive(Default)]
struct MigrationSummary {
    roles_failed: Vec<String>,
    /// Users created without any role because their source role(s) could not be mapped
    /// (e.g. system/built-in roles are not returned by the custom roles API).
    users_no_roles: Vec<(String, Vec<String>)>, // (login, source_role_ids as strings)
    users_failed: Vec<String>,
    /// Access groups successfully created on the destination (users included in payload).
    access_groups_created: Vec<String>,
    /// Access groups that already existed on the destination.
    /// N-central has no API to update group membership, so newly migrated users
    /// from the source were NOT added to these groups.
    access_groups_existed_not_updated: Vec<String>,
    access_groups_failed: Vec<String>,
}

// ==================== Main Migration Command ====================

#[tauri::command]
pub async fn start_migration(
    options: MigrationOptions,
    source_so_id: i64,
    dest_so_id: i64,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<ConnectionResult, String> {
    let source_client = state.client.lock().await;
    let dest_client = state.dest_client.lock().await;

    let (source, dest) = match (&*source_client, &*dest_client) {
        (Some(s), Some(d)) => (s, d),
        _ => return Err("Both source and destination must be connected".to_string()),
    };

    let mut mapping = IdMapping::new();
    let summary = MigrationSummary::default();

    // Always ensure the SO pair is in org_units, regardless of which options are selected.
    mapping.org_units.insert(source_so_id, dest_so_id);

    report_progress(&app_handle, "Migration", "Starting migration engine...", 0.0);
    emit_log(&app_handle, "info", "Starting migration engine...");

    // Get optional SOAP client for fallback operations
    let dest_soap = state.dest_soap_client.lock().await;
    let soap_ref = dest_soap.as_ref();

    // 1. Customers & Sites
    if options.customers {
        migrate_customers_and_sites(source, dest, source_so_id, dest_so_id, &mut mapping, soap_ref, &app_handle).await?;
    }

    // 2. User Roles (at all org unit levels)
    if options.user_roles {
        migrate_user_roles(source, dest, source_so_id, dest_so_id, &mut mapping, soap_ref, &app_handle).await?;
    }

    // 3. Users — must run BEFORE access groups so their IDs are available.
    // N-central has no API to update access group membership after creation,
    // so users must exist first and their IDs passed in the create payload.
    // Users are fetched from every level (SO, each customer, each site) and
    // created at the exact same org unit level they occupy on the source.
    if options.users {
        migrate_users(source, dest, source_so_id, dest_so_id, &mut mapping, soap_ref, &app_handle).await?;
    }

    // 4. Access Groups (at all org unit levels — after users so user IDs are available)
    if options.access_groups {
        migrate_access_groups(source, dest, source_so_id, dest_so_id, &mut mapping, soap_ref, &app_handle).await?;
    }

    // 5. Custom Properties
    if options.org_properties {
        migrate_org_properties(source, dest, source_so_id, &mapping, soap_ref, &app_handle).await?;
    }

    // ── Post-migration summary ──────────────────────────────────────────────────
    emit_log(&app_handle, "info", "─────────────── Migration Summary ───────────────");

    let has_issues = !summary.roles_failed.is_empty()
        || !summary.users_failed.is_empty()
        || !summary.users_no_roles.is_empty()
        || !summary.access_groups_existed_not_updated.is_empty()
        || !summary.access_groups_failed.is_empty();

    if !summary.roles_failed.is_empty() {
        emit_log(&app_handle, "warning", &format!(
            "Roles not created ({}) — missing description in source: {}",
            summary.roles_failed.len(),
            summary.roles_failed.join(", ")
        ));
    }

    if !summary.users_failed.is_empty() {
        emit_log(&app_handle, "error", &format!(
            "Users failed to create ({}): {}",
            summary.users_failed.len(),
            summary.users_failed.join(", ")
        ));
    }

    if !summary.users_no_roles.is_empty() {
        emit_log(&app_handle, "warning", &format!(
            "Users created without roles ({}) — source role is a system role not available via API:",
            summary.users_no_roles.len()
        ));
        for (login, roles) in &summary.users_no_roles {
            emit_log(&app_handle, "warning", &format!("  • {} (source roles: {})", login, roles.join(", ")));
        }
        emit_log(&app_handle, "warning", "  → Assign roles manually via Administration > User Management > Users.");
    }

    if !summary.access_groups_created.is_empty() {
        emit_log(&app_handle, "success", &format!(
            "Access groups created with all destination users included ({}): {}",
            summary.access_groups_created.len(),
            summary.access_groups_created.join(", ")
        ));
    }

    if !summary.access_groups_existed_not_updated.is_empty() {
        emit_log(&app_handle, "warning", &format!(
            "Access groups already on destination — newly migrated users NOT added ({}):",
            summary.access_groups_existed_not_updated.len()
        ));
        for name in &summary.access_groups_existed_not_updated {
            emit_log(&app_handle, "warning", &format!("  • {}", name));
        }
        emit_log(&app_handle, "warning", "  → N-central has no API to update existing group membership.");
        emit_log(&app_handle, "warning", "  → Add migrated users manually via Administration > Access Groups.");
    }

    if !summary.access_groups_failed.is_empty() {
        emit_log(&app_handle, "error", &format!(
            "Access groups failed to create ({}): {}",
            summary.access_groups_failed.len(),
            summary.access_groups_failed.join(", ")
        ));
    }

    if !has_issues {
        emit_log(&app_handle, "success", "All items migrated successfully — no action required.");
    }

    emit_log(&app_handle, "info", "─────────────────────────────────────────────────");
    // ────────────────────────────────────────────────────────────────────────────

    report_progress(&app_handle, "Complete", "Migration finished successfully", 100.0);

    Ok(ConnectionResult {
        success: true,
        message: "Migration completed successfully".to_string(),
        server_url: None,
        server_version: None,
        service_org_id: None,
        service_org_name: None,
    })
}
