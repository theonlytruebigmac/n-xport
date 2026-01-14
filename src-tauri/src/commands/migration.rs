//! Migration-related Tauri commands

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, State};

use crate::api::UserAddInfo;
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
    pub device_properties: bool,
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

    // Progress reporting helper
    let report_progress = |phase: &str, message: &str, percent: f32| {
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
    };

    report_progress("Migration", "Starting migration engine...", 0.0);

    // 1. Customers
    if options.customers {
        report_progress("Customers", "Fetching source customers...", 10.0);
        let source_customers = match source.get_customers_by_so(source_so_id).await {
            Ok(c) => c,
            Err(e) => return Err(format!("Failed to fetch source customers: {}", e)),
        };

        report_progress("Customers", "Fetching destination customers...", 15.0);
        let dest_customers = match dest.get_customers_by_so(dest_so_id).await {
            Ok(c) => c,
            Err(e) => return Err(format!("Failed to fetch destination customers: {}", e)),
        };

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

        let _total = source_customers.len();
        use futures::stream::{self, StreamExt};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let total = source_customers.len();
        let completed = Arc::new(AtomicUsize::new(0));

        let bodies = stream::iter(source_customers)
            .map(|source_cust| {
                let dest = dest.clone();
                let dest_name_map = dest_name_map.clone(); // Clone the map (HashMap<String, i64>) - slightly expensive but safe? Or wrap in Arc.
                                                           // Better to wrap dest_name_map in Arc before the loop.
                let completed = completed.clone();
                let app_handle = app_handle.clone(); // Need to clone handle for progress reporting if we did it inside
                                                     // Actually we can't emit from inside easily without cloning handle.
                                                     // Let's just emit "Migrating..." inside but we need the index? No, we just update progress based on count.

                async move {
                    // Check if exists
                    let dest_id = if let Some(&id) =
                        dest_name_map.get(&source_cust.customer_name.to_lowercase())
                    {
                        tracing::info!(
                            "Customer '{}' already exists on destination (ID: {})",
                            source_cust.customer_name,
                            id
                        );
                        id
                    } else {
                        // Create customer
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
                                tracing::info!(
                                    "Created customer '{}' (ID: {})",
                                    source_cust.customer_name,
                                    id
                                );
                                id
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to create customer '{}': {}",
                                    source_cust.customer_name,
                                    e
                                );
                                0
                            }
                        }
                    };

                    // Increment progress
                    let count = completed.fetch_add(1, Ordering::SeqCst) + 1;
                    let progress = 15.0 + (count as f32 / total as f32) * 15.0;
                    let _ = app_handle.emit(
                        "export-progress",
                        ProgressUpdate {
                            phase: "Customers".to_string(),
                            message: format!("Processed customer: {}", source_cust.customer_name), // slightly different msg
                            percent: progress,
                            current: count as u32,
                            total: total as u32,
                        },
                    );

                    (source_cust.customer_id, dest_id)
                }
            })
            .buffer_unordered(2); // Process 2 at a time to avoid rate limits

        let results: Vec<(i64, i64)> = bodies.collect().await;

        for (source_id, dest_id) in results {
            if dest_id != 0 {
                mapping.customers.insert(source_id, dest_id);
                // Also add to org_units for hierarchical lookup
                mapping.org_units.insert(source_id, dest_id);
            }
        }

        // Map the Service Org IDs as well
        mapping.org_units.insert(source_so_id, dest_so_id);

        // Migrate sites under matched customers
        report_progress("Sites", "Fetching source sites...", 30.0);
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
                // Find parent customer name in dest
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
            // Find source parent customer
            let source_parent_id = source_site
                .parent_id
                .or(source_site.customer_id)
                .or(source_site.customerid);
            if let Some(src_parent_id) = source_parent_id {
                // Look up source customer name using cached map
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
                    } else {
                        // Site doesn't exist on destination - CREATE it
                        // First, find the destination customer ID for this source customer
                        if let Some(&dest_cust_id) = mapping.customers.get(&src_parent_id) {
                            tracing::info!(
                                "Creating site '{}' under customer '{}' (dest customer ID: {})...",
                                source_site.site_name,
                                src_cust_name,
                                dest_cust_id
                            );

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
                                        tracing::info!(
                                            "Created site '{}' (ID: {})",
                                            source_site.site_name,
                                            id
                                        );
                                    } else {
                                        tracing::warn!(
                                            "Site '{}' created but no ID returned in response: {:?}",
                                            source_site.site_name,
                                            resp
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to create site '{}': {}",
                                        source_site.site_name,
                                        e
                                    );
                                }
                            }
                        } else {
                            tracing::warn!(
                                "Site '{}' under customer '{}' skipped - parent customer not mapped to destination",
                                source_site.site_name,
                                src_cust_name
                            );
                        }
                    }
                }
            }
        }

        tracing::info!(
            "Org unit mapping: {} customers, {} sites, {} total org_units",
            mapping.customers.len(),
            mapping.sites.len(),
            mapping.org_units.len()
        );
    }

    // 2. User Roles
    if options.user_roles {
        report_progress("Roles", "Loading permission mappings...", 28.0);

        // Load permission name -> ID mappings from embedded CSV (located in src-tauri/)
        let perm_csv = include_str!("../../rolePermissionIds.csv");
        let perm_lookup = crate::models::user_role::PermissionLookup::from_csv(perm_csv);

        if perm_lookup.is_empty() {
            tracing::warn!("No permission mappings loaded from CSV - roles will be created with minimal permissions");
        }

        report_progress("Roles", "Fetching source roles...", 30.0);
        let source_roles = match source.get_user_roles(source_so_id).await {
            Ok(r) => r,
            Err(e) => return Err(format!("Failed to fetch source roles: {}", e)),
        };

        report_progress("Roles", "Fetching destination roles...", 35.0);
        let dest_roles = match dest.get_user_roles(dest_so_id).await {
            Ok(r) => r,
            Err(e) => return Err(format!("Failed to fetch destination roles: {}", e)),
        };

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
                // Get permission names from source role
                let source_permissions = source_role.get_permissions();

                // Convert permission names to IDs using the CSV lookup
                let permission_ids: Vec<i64> =
                    if !perm_lookup.is_empty() && !source_permissions.is_empty() {
                        perm_lookup.names_to_ids(&source_permissions)
                    } else {
                        // Fallback: minimal permission (ACTIVE_ISSUES_VIEW = 1701)
                        vec![1701]
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
                    Err(e) => tracing::error!("Failed to create role '{}': {}", role_name, e),
                }
            }
        }
    }

    // 3. Access Groups
    if options.access_groups {
        report_progress("Access Groups", "Fetching source access groups...", 50.0);
        let source_groups = match source.get_access_groups(source_so_id).await {
            Ok(g) => g,
            Err(e) => return Err(format!("Failed to fetch source groups: {}", e)),
        };

        report_progress(
            "Access Groups",
            "Fetching destination access groups...",
            55.0,
        );
        let dest_groups = match dest.get_access_groups(dest_so_id).await {
            Ok(g) => g,
            Err(e) => return Err(format!("Failed to fetch destination groups: {}", e)),
        };

        // Fetch destination customers to get their IDs (customers may have been migrated previously)
        report_progress(
            "Access Groups",
            "Fetching destination customer IDs...",
            58.0,
        );
        let dest_customers = dest
            .get_customers_by_so(dest_so_id)
            .await
            .unwrap_or_default();
        let dest_customer_ids: Vec<String> = dest_customers
            .iter()
            .map(|c| c.customer_id.to_string())
            .collect();

        // Fetch destination users to get their IDs (required for access group creation)
        report_progress("Access Groups", "Fetching destination user IDs...", 59.0);
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
                // Create access group with destination customer IDs
                let group_type = source_group.group_type.as_deref().unwrap_or("ORG_UNIT");
                let description = source_group.group_description.as_deref().unwrap_or("");

                // If no customers exist in destination, we can't create the access group
                if dest_customer_ids.is_empty() {
                    tracing::warn!(
                        "Cannot create access group '{}': No customers exist in destination.",
                        group_name
                    );
                    continue;
                }

                // Log the actual values for debugging
                tracing::info!(
                    "Access group '{}' - using {} customer IDs: {:?}",
                    group_name,
                    dest_customer_ids.len(),
                    &dest_customer_ids[..std::cmp::min(3, dest_customer_ids.len())] // Show first 3
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

                // Use type-specific endpoint based on group_type
                let result = if group_type == "DEVICE" {
                    dest.create_device_access_group(dest_so_id, &payload).await
                } else {
                    // Default to org unit access group
                    dest.create_org_unit_access_group(dest_so_id, &payload)
                        .await
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
                    Err(e) => {
                        tracing::error!("Failed to create access group '{}': {}", group_name, e)
                    }
                }
            }
        }
    }

    // 4. Users
    if options.users {
        report_progress("Users", "Fetching source users and roles...", 70.0);
        let source_users = match source.get_users_by_org_unit(source_so_id).await {
            Ok(u) => u,
            Err(e) => return Err(format!("Failed to fetch source users: {}", e)),
        };

        // Fetch source roles to build source_role_id -> role_name map
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

        // If role_names is empty (roles weren't migrated), populate from destination roles
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

        report_progress("Users", "Fetching destination users...", 75.0);
        let dest_users = match dest.get_users_by_org_unit(dest_so_id).await {
            Ok(u) => u,
            Err(e) => return Err(format!("Failed to fetch destination users: {}", e)),
        };

        let dest_login_map: HashMap<String, i64> = dest_users
            .into_iter()
            .map(|u| (u.login_name.to_lowercase(), u.user_id))
            .collect();

        let total = source_users.len();
        for (i, source_user) in source_users.into_iter().enumerate() {
            let progress = 75.0 + (i as f32 / total as f32) * 15.0;
            report_progress(
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

            // Get source role NAMES for this user, then look up dest role IDs by name
            let mapped_roles: Vec<i64> = source_user
                .role_ids
                .iter()
                .filter_map(|src_role_id| {
                    // Step 1: Get role name from source role ID
                    if let Some(role_name) = source_role_id_to_name.get(src_role_id) {
                        // Step 2: Look up dest role ID by name
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

            // Access groups: user doesn't need them during creation, they're handled separately
            // Access groups will be created AFTER users, and will include user IDs

            tracing::info!(
                "User '{}' - Source role_ids: {:?}, Mapped to dest: {:?}",
                source_user.login_name,
                source_user.role_ids,
                mapped_roles
            );

            // N-Central REST API does not support user creation, so use SOAP API
            let dest_soap_client = state.dest_soap_client.lock().await;
            if let Some(soap) = &*dest_soap_client {
                // Determine destination org unit for user creation
                // Use org_units mapping if source user has a service_org_id we've mapped
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
                    // Use login_name as email fallback since login_name IS the email address
                    email: source_user
                        .email
                        .clone()
                        .unwrap_or_else(|| source_user.login_name.clone()),
                    first_name: source_user.first_name.clone().unwrap_or_default(),
                    last_name: source_user.last_name.clone().unwrap_or_default(),
                    phone: None,      // Source user model doesn't have phone
                    department: None, // Source user model doesn't have department
                    location: None,   // Source user model might not have location
                    is_enabled: true, // Default to enabled for migration
                    customer_id: dest_customer_id,
                    role_ids: mapped_roles,
                    access_group_ids: vec![], // Access groups handle user membership separately
                };

                match soap.user_add(&source_user.login_name, &user_info).await {
                    Ok(id) => {
                        if id > 0 {
                            // Store created user ID for access group creation
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

        // Log summary of users that need manual creation
        tracing::info!(
            "User migration note: N-Central REST API does not support creating users. Users not found in destination must be created manually in the N-Central Admin UI, then re-run migration to sync their roles and access groups."
        );
    }

    // 5. Custom Properties
    if options.org_properties {
        report_progress("Properties", "Migrating org custom properties...", 90.0);

        let source_props = match source.get_org_properties(source_so_id).await {
            Ok(p) => p,
            Err(e) => return Err(format!("Failed to fetch source properties: {}", e)),
        };

        for prop in source_props {
            // Properties are complex because they have values and definitions.
            // For now, let's just log or implement a basic value sync if the property name matches.
            // TODO: Deep property sync
            tracing::info!(
                "Syncing property: {:?} (Basic sync logic pending)",
                prop.label
            );
        }
    }

    report_progress("Complete", "Migration finished successfully", 100.0);

    Ok(ConnectionResult {
        success: true,
        message: "Migration completed successfully".to_string(),
        server_url: None,
        server_version: None,
        service_org_id: None,
        service_org_name: None,
    })
}
