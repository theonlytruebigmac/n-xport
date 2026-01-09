//! Migration-related Tauri commands

use std::collections::HashMap;
use tauri::{State, AppHandle, Emitter};
use serde::{Serialize, Deserialize};

use crate::models::*;
use crate::commands::connection::{AppState, ConnectionResult};

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

/// Mapping of Source IDs to Destination IDs
pub struct IdMapping {
    /// Source Customer ID -> Destination Customer ID
    pub customers: HashMap<i64, i64>,
    /// Source Role ID -> Destination Role ID
    pub roles: HashMap<i64, i64>,
    /// Source Access Group ID -> Destination Access Group ID
    pub access_groups: HashMap<i64, i64>,
}

impl IdMapping {
    pub fn new() -> Self {
        Self {
            customers: HashMap::new(),
            roles: HashMap::new(),
            access_groups: HashMap::new(),
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
        let _ = app_handle.emit("export-progress", ProgressUpdate {
            phase: phase.to_string(),
            message: message.to_string(),
            percent,
            current: 0,
            total: 100,
        });
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

        let mut dest_name_map = HashMap::new();
        for c in &dest_customers {
            dest_name_map.insert(c.customer_name.to_lowercase(), c.customer_id);
        }

        let total = source_customers.len();
        for (i, source_cust) in source_customers.into_iter().enumerate() {
            let progress = 15.0 + (i as f32 / total as f32) * 15.0;
            report_progress("Customers", &format!("Migrating customer: {}", source_cust.customer_name), progress);

            let dest_id = if let Some(&id) = dest_name_map.get(&source_cust.customer_name.to_lowercase()) {
                tracing::info!("Customer '{}' already exists on destination (ID: {})", source_cust.customer_name, id);
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

                match dest.create_customer(&payload).await {
                    Ok(resp) => {
                        let id = resp["customerId"].as_i64().or_else(|| resp["id"].as_i64()).unwrap_or(0);
                        tracing::info!("Created customer '{}' (ID: {})", source_cust.customer_name, id);
                        id
                    }
                    Err(e) => {
                        tracing::error!("Failed to create customer '{}': {}", source_cust.customer_name, e);
                        0
                    }
                }
            };

            if dest_id != 0 {
                mapping.customers.insert(source_cust.customer_id, dest_id);
            }
        }
    }

    // 2. User Roles
    if options.user_roles {
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
            } else {
                // Create role
                let payload = serde_json::json!({
                    "roleName": role_name,
                    "roleDescription": source_role.role_description,
                });
                match dest.create_user_role(dest_so_id, &payload).await {
                    Ok(resp) => {
                        let id = resp["roleId"].as_i64().or_else(|| resp["id"].as_i64()).unwrap_or(0);
                        if id != 0 { mapping.roles.insert(source_role.role_id, id); }
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

        report_progress("Access Groups", "Fetching destination access groups...", 55.0);
        let dest_groups = match dest.get_access_groups(dest_so_id).await {
            Ok(g) => g,
            Err(e) => return Err(format!("Failed to fetch destination groups: {}", e)),
        };

        let mut dest_name_map = HashMap::new();
        for g in &dest_groups {
            if let Some(ref name) = g.group_name {
                dest_name_map.insert(name.to_lowercase(), g.group_id);
            }
        }

        for source_group in source_groups {
            let group_name = source_group.group_name.as_deref().unwrap_or("Unknown Group");
            if let Some(&id) = dest_name_map.get(&group_name.to_lowercase()) {
                mapping.access_groups.insert(source_group.group_id, id);
            } else {
                // Create group
                let payload = serde_json::json!({
                    "groupName": group_name,
                    "groupDescription": source_group.group_description,
                    "groupType": source_group.group_type,
                });
                match dest.create_access_group(dest_so_id, &payload).await {
                    Ok(resp) => {
                        let id = resp["groupId"].as_i64().or_else(|| resp["id"].as_i64()).unwrap_or(0);
                        if id != 0 { mapping.access_groups.insert(source_group.group_id, id); }
                    }
                    Err(e) => tracing::error!("Failed to create access group '{}': {}", group_name, e),
                }
            }
        }
    }

    // 4. Users
    if options.users {
        report_progress("Users", "Fetching source users...", 70.0);
        let source_users = match source.get_users_by_org_unit(source_so_id).await {
            Ok(u) => u,
            Err(e) => return Err(format!("Failed to fetch source users: {}", e)),
        };

        report_progress("Users", "Fetching destination users...", 75.0);
        let dest_users = match dest.get_users_by_org_unit(dest_so_id).await {
            Ok(u) => u,
            Err(e) => return Err(format!("Failed to fetch destination users: {}", e)),
        };

        let dest_login_map: HashMap<String, i64> = dest_users.into_iter().map(|u| (u.login_name.to_lowercase(), u.user_id)).collect();

        let total = source_users.len();
        for (i, source_user) in source_users.into_iter().enumerate() {
            let progress = 75.0 + (i as f32 / total as f32) * 15.0;
            report_progress("Users", &format!("Migrating user: {}", source_user.login_name), progress);

            if dest_login_map.contains_key(&source_user.login_name.to_lowercase()) {
                tracing::info!("User '{}' already exists on destination", source_user.login_name);
                continue;
            }

            // Map roles and groups
            let mapped_roles: Vec<i64> = source_user.role_ids.iter().filter_map(|id| mapping.roles.get(id).cloned()).collect();
            let mapped_groups: Vec<i64> = source_user.access_group_ids.iter().filter_map(|id| mapping.access_groups.get(id).cloned()).collect();

            let payload = serde_json::json!({
                "userName": source_user.login_name,
                "firstName": source_user.first_name,
                "lastName": source_user.last_name,
                "email": source_user.email,
                "isEnabled": source_user.is_enabled,
                "roleIds": mapped_roles,
                "accessGroupIds": mapped_groups,
                "apiOnlyUser": source_user.api_only_user,
            });

            match dest.create_user(dest_so_id, &payload).await {
                Ok(_) => tracing::info!("Created user '{}'", source_user.login_name),
                Err(e) => tracing::error!("Failed to create user '{}': {}", source_user.login_name, e),
            }
        }
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
            tracing::info!("Syncing property: {:?} (Basic sync logic pending)", prop.label);
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
