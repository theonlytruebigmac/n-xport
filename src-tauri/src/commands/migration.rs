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
    /// Source Role ID -> Destination Role ID (legacy, kept for reference)
    pub roles: HashMap<i64, i64>,
    /// Source Access Group ID -> Destination Access Group ID
    pub access_groups: HashMap<i64, i64>,
    /// Role Name (lowercase) -> Destination Role ID (for user creation)
    pub role_names: HashMap<String, i64>,
    /// User Login Name (lowercase) -> Destination User ID (for access group creation)
    pub user_logins: HashMap<String, i64>,
    /// Source Org Unit ID -> Destination Org Unit ID (SO, Customer, or Site)
    pub org_units: HashMap<i64, i64>,
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
                            let id = resp["customerId"]
                                .as_i64()
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
                    tracing::debug!(
                        "Site '{}' under customer '{}' mapped to dest ID {}",
                        source_site.site_name,
                        src_cust_name,
                        dest_site_id
                    );
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
                            let id = resp["siteId"]
                                .as_i64()
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

/// Migrate user roles from source to destination.
/// Populates `mapping.roles` and `mapping.role_names`.
async fn migrate_user_roles(
    source: &NcClient,
    dest: &NcClient,
    source_so_id: i64,
    dest_so_id: i64,
    mapping: &mut IdMapping,
    soap_client: Option<&NcSoapClient>,
    app_handle: &AppHandle,
) -> Result<(), String> {
    report_progress(app_handle, "Roles", "Loading permission mappings...", 28.0);

    let perm_csv = include_str!("../../rolePermissionIds.csv");
    let perm_lookup = crate::models::user_role::PermissionLookup::from_csv(perm_csv);

    if perm_lookup.is_empty() {
        tracing::warn!("No permission mappings loaded from CSV - roles will be created with minimal permissions");
    }

    report_progress(app_handle, "Roles", "Fetching source roles...", 30.0);
    let source_roles = source
        .get_user_roles(source_so_id)
        .await
        .map_err(|e| format!("Failed to fetch source roles: {}", e))?;

    report_progress(app_handle, "Roles", "Fetching destination roles...", 35.0);
    let dest_roles = dest
        .get_user_roles(dest_so_id)
        .await
        .map_err(|e| format!("Failed to fetch destination roles: {}", e))?;

    let mut dest_name_map = HashMap::new();
    for r in &dest_roles {
        if let Some(ref name) = r.role_name {
            dest_name_map.insert(name.to_lowercase(), r.role_id);
        }
    }

    for source_role in source_roles {
        let role_name = source_role.role_name.as_deref().unwrap_or("Unknown Role");
        if let Some(&id) = dest_name_map.get(&role_name.to_lowercase()) {
            mapping.roles.insert(source_role.role_id, id);
            mapping.role_names.insert(role_name.to_lowercase(), id);
            tracing::debug!(
                "Role '{}' already exists in destination (ID: {})",
                role_name,
                id
            );
        } else {
            let source_permissions = source_role.get_permissions();
            let permission_ids: Vec<i64> =
                if !perm_lookup.is_empty() && !source_permissions.is_empty() {
                    perm_lookup.names_to_ids(&source_permissions)
                } else {
                    vec![1701] // Fallback: ACTIVE_ISSUES_VIEW
                };

            let description = source_role
                .role_description
                .as_deref()
                .unwrap_or("Migrated role");

            let payload = serde_json::json!({
                "roleName": role_name,
                "description": description,
                "permissionIds": permission_ids,
                "userIds": []
            });

            tracing::info!(
                "Creating role '{}' with {} permissions...",
                role_name,
                permission_ids.len()
            );

            match dest.create_user_role(dest_so_id, &payload).await {
                Ok(resp) => {
                    tracing::info!("Role creation response: {:?}", resp);
                    let id = resp["roleId"]
                        .as_i64()
                        .or_else(|| resp["id"].as_i64())
                        .unwrap_or(0);
                    if id != 0 {
                        mapping.roles.insert(source_role.role_id, id);
                        mapping.role_names.insert(role_name.to_lowercase(), id);
                        tracing::info!(
                            "Created role '{}' (source ID: {} → dest ID: {}) with {} permissions",
                            role_name,
                            source_role.role_id,
                            id,
                            permission_ids.len()
                        );
                    } else {
                        tracing::warn!(
                            "Role '{}' created but no ID returned in response",
                            role_name
                        );
                    }
                }
                Err(rest_err) => {
                    // REST failed — try SOAP fallback
                    tracing::warn!("REST create_user_role failed for '{}': {}. Trying SOAP...", role_name, rest_err);
                    if let Some(soap) = soap_client {
                        match soap.user_role_add(role_name, description, dest_so_id, &permission_ids).await {
                            Ok(id) => {
                                if id != 0 {
                                    mapping.roles.insert(source_role.role_id, id);
                                    mapping.role_names.insert(role_name.to_lowercase(), id);
                                    tracing::info!("Created role '{}' via SOAP (ID: {})", role_name, id);
                                }
                            }
                            Err(soap_err) => {
                                tracing::error!("Failed to create role '{}' (REST: {}, SOAP: {})", role_name, rest_err, soap_err);
                            }
                        }
                    } else {
                        tracing::error!("Failed to create role '{}': {}", role_name, rest_err);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Migrate access groups from source to destination.
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
    report_progress(app_handle, "Access Groups", "Fetching source access groups...", 50.0);
    let source_groups = source
        .get_access_groups(source_so_id)
        .await
        .map_err(|e| format!("Failed to fetch source groups: {}", e))?;

    report_progress(app_handle, "Access Groups", "Fetching destination access groups...", 55.0);
    let dest_groups = dest
        .get_access_groups(dest_so_id)
        .await
        .map_err(|e| format!("Failed to fetch destination groups: {}", e))?;

    report_progress(app_handle, "Access Groups", "Fetching destination customer IDs...", 58.0);
    let dest_customers = dest
        .get_customers_by_so(dest_so_id)
        .await
        .unwrap_or_default();
    let dest_customer_ids: Vec<String> = dest_customers
        .iter()
        .map(|c| c.customer_id.to_string())
        .collect();

    report_progress(app_handle, "Access Groups", "Fetching destination user IDs...", 59.0);
    let dest_users = dest
        .get_users_by_org_unit(dest_so_id)
        .await
        .unwrap_or_default();
    let dest_user_ids: Vec<String> = dest_users.iter().map(|u| u.user_id.to_string()).collect();

    tracing::info!(
        "Found {} destination customers and {} users for access group creation",
        dest_customer_ids.len(),
        dest_user_ids.len()
    );

    let mut dest_name_map = HashMap::new();
    for g in &dest_groups {
        if let Some(ref name) = g.group_name {
            dest_name_map.insert(name.to_lowercase(), g.group_id);
        }
    }

    for source_group in source_groups {
        let group_name = source_group
            .group_name
            .as_deref()
            .unwrap_or("Unknown Group");
        if let Some(&id) = dest_name_map.get(&group_name.to_lowercase()) {
            mapping.access_groups.insert(source_group.group_id, id);
            tracing::debug!(
                "Access group '{}' already exists in destination (ID: {})",
                group_name,
                id
            );
        } else {
            let group_type = source_group.group_type.as_deref().unwrap_or("ORG_UNIT");
            let description = source_group.group_description.as_deref().unwrap_or("");

            if dest_customer_ids.is_empty() {
                tracing::warn!(
                    "Cannot create access group '{}': No customers exist in destination.",
                    group_name
                );
                continue;
            }

            tracing::info!(
                "Access group '{}' - using {} customer IDs: {:?}",
                group_name,
                dest_customer_ids.len(),
                &dest_customer_ids[..std::cmp::min(3, dest_customer_ids.len())]
            );

            let payload = serde_json::json!({
                "groupName": group_name,
                "groupDescription": description,
                "orgUnitIds": dest_customer_ids,
                "userIds": dest_user_ids,
                "autoIncludeNewOrgUnits": "true"
            });

            tracing::info!(
                "Creating access group '{}' (type: {}) with {} org units and {} users...",
                group_name,
                group_type,
                dest_customer_ids.len(),
                dest_user_ids.len()
            );
            tracing::debug!(
                "Payload: {}",
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            );

            let result = if group_type == "DEVICE" {
                dest.create_device_access_group(dest_so_id, &payload).await
            } else {
                dest.create_org_unit_access_group(dest_so_id, &payload).await
            };

            match result {
                Ok(resp) => {
                    let id = resp["groupId"]
                        .as_i64()
                        .or_else(|| resp["id"].as_i64())
                        .unwrap_or(0);
                    if id != 0 {
                        mapping.access_groups.insert(source_group.group_id, id);
                        tracing::info!("Created access group '{}' (ID: {})", group_name, id);
                    }
                }
                Err(rest_err) => {
                    // REST failed — try SOAP fallback
                    tracing::warn!("REST create access group failed for '{}': {}. Trying SOAP...", group_name, rest_err);
                    if let Some(soap) = soap_client {
                        match soap.access_group_add(group_name, description, dest_so_id, group_type, true).await {
                            Ok(id) => {
                                if id != 0 {
                                    mapping.access_groups.insert(source_group.group_id, id);
                                    tracing::info!("Created access group '{}' via SOAP (ID: {})", group_name, id);
                                }
                            }
                            Err(soap_err) => {
                                tracing::error!("Failed to create access group '{}' (REST: {}, SOAP: {})", group_name, rest_err, soap_err);
                            }
                        }
                    } else {
                        tracing::error!("Failed to create access group '{}': {}", group_name, rest_err);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Migrate users from source to destination via SOAP API.
/// Populates `mapping.user_logins`.
async fn migrate_users(
    source: &NcClient,
    dest: &NcClient,
    source_so_id: i64,
    dest_so_id: i64,
    mapping: &mut IdMapping,
    state: &State<'_, AppState>,
    app_handle: &AppHandle,
) -> Result<(), String> {
    report_progress(app_handle, "Users", "Fetching source users and roles...", 70.0);
    let source_users = source
        .get_users_by_org_unit(source_so_id)
        .await
        .map_err(|e| format!("Failed to fetch source users: {}", e))?;

    // Build source role_id -> role_name map
    let source_roles = source
        .get_user_roles(source_so_id)
        .await
        .unwrap_or_default();
    let source_role_id_to_name: HashMap<i64, String> = source_roles
        .iter()
        .filter_map(|r| {
            r.role_name
                .as_ref()
                .map(|name| (r.role_id, name.to_lowercase()))
        })
        .collect();

    // If role_names is empty (roles weren't migrated), populate from destination
    if mapping.role_names.is_empty() {
        tracing::info!("Role names map is empty - fetching destination roles to populate...");
        let dest_roles = dest.get_user_roles(dest_so_id).await.unwrap_or_default();
        for r in dest_roles {
            if let Some(name) = r.role_name {
                mapping.role_names.insert(name.to_lowercase(), r.role_id);
            }
        }
        tracing::info!(
            "Populated role_names from destination with {} roles",
            mapping.role_names.len()
        );
    }

    tracing::info!(
        "Built source role ID→name map with {} entries. role_names map has {} entries.",
        source_role_id_to_name.len(),
        mapping.role_names.len()
    );

    report_progress(app_handle, "Users", "Fetching destination users...", 75.0);
    let dest_users = dest
        .get_users_by_org_unit(dest_so_id)
        .await
        .map_err(|e| format!("Failed to fetch destination users: {}", e))?;

    let dest_login_map: HashMap<String, i64> = dest_users
        .into_iter()
        .map(|u| (u.login_name.to_lowercase(), u.user_id))
        .collect();

    let total = source_users.len();
    for (i, source_user) in source_users.into_iter().enumerate() {
        let progress = 75.0 + (i as f32 / total as f32) * 15.0;
        report_progress(
            app_handle,
            "Users",
            &format!("Migrating user: {}", source_user.login_name),
            progress,
        );

        if dest_login_map.contains_key(&source_user.login_name.to_lowercase()) {
            tracing::info!(
                "User '{}' already exists on destination",
                source_user.login_name
            );
            continue;
        }

        // Map source role IDs -> dest role IDs via role names
        let mapped_roles: Vec<i64> = source_user
            .role_ids
            .iter()
            .filter_map(|src_role_id| {
                if let Some(role_name) = source_role_id_to_name.get(src_role_id) {
                    if let Some(&dest_role_id) = mapping.role_names.get(role_name) {
                        tracing::debug!(
                            "Mapped role: source_id {} -> name '{}' -> dest_id {}",
                            src_role_id,
                            role_name,
                            dest_role_id
                        );
                        return Some(dest_role_id);
                    }
                }
                tracing::warn!(
                    "Could not map source role ID {} to destination for user '{}'",
                    src_role_id,
                    source_user.login_name
                );
                None
            })
            .collect();

        tracing::info!(
            "User '{}' - Source role_ids: {:?}, Mapped to dest: {:?}",
            source_user.login_name,
            source_user.role_ids,
            mapped_roles
        );

        // Use SOAP API for user creation (REST API doesn't support it)
        let dest_soap_client = state.dest_soap_client.lock().await;
        if let Some(soap) = &*dest_soap_client {
            let dest_customer_id = source_user
                .service_org_id
                .and_then(|src_org_id| mapping.org_units.get(&src_org_id).cloned())
                .unwrap_or(dest_so_id);

            tracing::info!(
                "Creating user '{}' via SOAP API at customerID {} (source org: {:?})...",
                source_user.login_name,
                dest_customer_id,
                source_user.service_org_id
            );

            let user_info = UserAddInfo {
                email: source_user
                    .email
                    .clone()
                    .unwrap_or_else(|| source_user.login_name.clone()),
                first_name: source_user.first_name.clone().unwrap_or_default(),
                last_name: source_user.last_name.clone().unwrap_or_default(),
                phone: None,
                department: None,
                location: None,
                is_enabled: true,
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
                        tracing::info!(
                            "Created user '{}' (ID: {})",
                            source_user.login_name,
                            id
                        );
                    } else {
                        tracing::warn!(
                            "User '{}' returned ID {} - may already exist elsewhere",
                            source_user.login_name,
                            id
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to create user '{}' via SOAP: {}",
                        source_user.login_name,
                        e
                    );
                }
            }
        } else {
            tracing::warn!(
                "SOAP client not initialized - cannot create user '{}'. Manual creation required.",
                source_user.login_name
            );
        }
    }

    tracing::info!(
        "User migration note: N-Central REST API does not support creating users. \
         Users not found in destination must be created manually in the N-Central Admin UI, \
         then re-run migration to sync their roles and access groups."
    );

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
                    tracing::info!(
                        "Property {:?} belongs to source SO root, skipping",
                        prop.label
                    );
                    skipped += 1;
                    continue;
                } else {
                    tracing::warn!(
                        "No mapping for source org unit {} (property: {:?}), skipping",
                        src_ou_id,
                        prop.label
                    );
                    skipped += 1;
                    continue;
                }
            }
            None => {
                tracing::warn!("Property {:?} has no org_unit_id, skipping", prop.label);
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
            }
            Err(rest_err) => {
                // REST failed — try SOAP fallback
                if let Some(soap) = soap_client {
                    let prop_value = prop.value.as_deref().unwrap_or("");
                    match soap.organization_property_modify(dest_ou_id, prop.property_id, prop_value).await {
                        Ok(_) => {
                            synced += 1;
                            tracing::info!("Synced property {:?} via SOAP fallback", prop.label);
                        }
                        Err(soap_err) => {
                            tracing::warn!(
                                "Failed to sync property {:?} to OU {} (REST: {}, SOAP: {})",
                                prop.label, dest_ou_id, rest_err, soap_err
                            );
                            failed += 1;
                        }
                    }
                } else {
                    tracing::warn!(
                        "Failed to sync property {:?} to OU {}: {}",
                        prop.label, dest_ou_id, rest_err
                    );
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

    report_progress(&app_handle, "Migration", "Starting migration engine...", 0.0);
    emit_log(&app_handle, "info", "Starting migration engine...");

    // Get optional SOAP client for fallback operations
    let dest_soap = state.dest_soap_client.lock().await;
    let soap_ref = dest_soap.as_ref();

    // 1. Customers & Sites
    if options.customers {
        migrate_customers_and_sites(source, dest, source_so_id, dest_so_id, &mut mapping, soap_ref, &app_handle).await?;
    }

    // 2. User Roles
    if options.user_roles {
        migrate_user_roles(source, dest, source_so_id, dest_so_id, &mut mapping, soap_ref, &app_handle).await?;
    }

    // 3. Access Groups
    if options.access_groups {
        migrate_access_groups(source, dest, source_so_id, dest_so_id, &mut mapping, soap_ref, &app_handle).await?;
    }

    // 4. Users
    if options.users {
        migrate_users(source, dest, source_so_id, dest_so_id, &mut mapping, &state, &app_handle).await?;
    }

    // 5. Custom Properties
    if options.org_properties {
        migrate_org_properties(source, dest, source_so_id, &mapping, soap_ref, &app_handle).await?;
    }

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
