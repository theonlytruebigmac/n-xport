//! Per-resource import handlers. Each handler:
//!  - parses one row from the CSV
//!  - resolves any name-based foreign keys against the connected source server
//!  - either creates the resource (live) or reports what would be created (dry run)
//!  - returns a RowOutcome that the caller logs and tallies in the summary.
//!
//! Handlers prefer REST and fall back to SOAP when the REST endpoint isn't
//! available (e.g. user creation is SOAP-only on N-central).

use std::collections::HashMap;

use serde_json::json;
use tauri::{AppHandle, Emitter};

use super::{
    AccessGroupImportRow, CustomerImportRow, RowOutcome, RowStatus, SiteImportRow,
    UserImportRow, UserRoleImportRow,
};
use crate::api::{NcClient, NcSoapClient, UserAddInfo};
use crate::models::*;

/// Pre-fetched lookup data needed across most handlers.
pub struct ImportContext {
    pub source_so_id: i64,
    /// customer name (lowercase) -> customer id
    pub customers_by_name: HashMap<String, i64>,
    /// (customer name lowercase, site name lowercase) -> site id
    pub sites_by_name: HashMap<(String, String), i64>,
    /// role name (lowercase) -> role id (any OU; first-seen wins)
    pub roles_by_name: HashMap<String, i64>,
    /// group name (lowercase) -> group id (any OU; first-seen wins)
    pub groups_by_name: HashMap<String, i64>,
    /// permission name -> permission id (loaded from rolePermissionIds.csv)
    pub permission_lookup: PermissionLookup,
}

impl ImportContext {
    /// Build the lookup tables for the target service org.
    pub async fn load(client: &NcClient, source_so_id: i64) -> Result<Self, String> {
        let mut customers_by_name = HashMap::new();
        let customers = client
            .get_customers_by_so(source_so_id)
            .await
            .map_err(|e| format!("Failed to fetch customers: {}", e))?;
        for c in &customers {
            customers_by_name.insert(c.customer_name.to_lowercase(), c.customer_id);
        }

        // Build (customer_name, site_name) -> site_id map. Sites only carry parent IDs,
        // so we resolve the parent customer's name from the just-fetched customer list.
        let mut sites_by_name = HashMap::new();
        let cust_id_to_name: HashMap<i64, String> = customers
            .iter()
            .map(|c| (c.customer_id, c.customer_name.to_lowercase()))
            .collect();
        if let Ok(sites) = client.get_sites_by_so(source_so_id).await {
            for s in sites {
                let parent_id = s.parent_id.or(s.customer_id).or(s.customerid);
                if let Some(pid) = parent_id {
                    if let Some(parent_name) = cust_id_to_name.get(&pid) {
                        sites_by_name.insert(
                            (parent_name.clone(), s.site_name.to_lowercase()),
                            s.site_id,
                        );
                    }
                }
            }
        }

        // Roles & groups: walk SO + every customer to populate name -> id maps.
        // First-seen wins because inherited roles appear at every child OU.
        let mut roles_by_name = HashMap::new();
        let mut groups_by_name = HashMap::new();
        let mut all_ous: Vec<i64> = vec![source_so_id];
        all_ous.extend(customers.iter().map(|c| c.customer_id));
        for ou_id in &all_ous {
            if let Ok(roles) = client.get_user_roles(*ou_id).await {
                for r in roles {
                    if let Some(name) = r.role_name {
                        roles_by_name
                            .entry(name.to_lowercase())
                            .or_insert(r.role_id);
                    }
                }
            }
            if let Ok(groups) = client.get_access_groups(*ou_id).await {
                for g in groups {
                    if let Some(name) = g.group_name {
                        groups_by_name
                            .entry(name.to_lowercase())
                            .or_insert(g.group_id);
                    }
                }
            }
        }

        // Load permission name->id lookup from the bundled CSV (same one migration uses)
        let perm_csv = include_str!("../../rolePermissionIds.csv");
        let permission_lookup = PermissionLookup::from_csv(perm_csv);

        Ok(Self {
            source_so_id,
            customers_by_name,
            sites_by_name,
            roles_by_name,
            groups_by_name,
            permission_lookup,
        })
    }
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

/// Helper: extract created resource id from an N-central JSON response,
/// trying common envelope shapes.
fn extract_id(resp: &serde_json::Value, keys: &[&str]) -> i64 {
    let data = if resp.get("data").is_some() {
        &resp["data"]
    } else {
        resp
    };
    for k in keys {
        if let Some(id) = data.get(k).and_then(|v| v.as_i64()) {
            return id;
        }
        if let Some(id) = resp.get(k).and_then(|v| v.as_i64()) {
            return id;
        }
    }
    0
}

// ==================== Customers ====================

pub async fn import_customer(
    row_number: usize,
    row: CustomerImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    soap: Option<&NcSoapClient>,
    dry_run: bool,
    app: &AppHandle,
) -> RowOutcome {
    let name = row.customer_name.trim();
    if name.is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: format!("(row {})", row_number),
            message: "customerName is required".into(),
        };
    }
    let key = name.to_lowercase();

    if let Some(&existing_id) = ctx.customers_by_name.get(&key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: name.to_string(),
            message: format!("Customer already exists (ID: {})", existing_id),
        };
    }

    if dry_run {
        return RowOutcome {
            row_number,
            status: RowStatus::Planned,
            label: name.to_string(),
            message: format!("Would create customer under SO {}", ctx.source_so_id),
        };
    }

    let payload = json!({
        "customerName": name,
        "parentId": ctx.source_so_id,
        "externalId": row.external_id,
        "contactFirstName": row.contact_first_name,
        "contactLastName": row.contact_last_name,
        "contactEmail": row.contact_email,
        "contactPhone": row.contact_phone,
        "street1": row.street1,
        "street2": row.street2,
        "city": row.city,
        "stateProv": row.state_prov,
        "country": row.country,
        "postalCode": row.postal_code,
    });

    match client.create_customer(ctx.source_so_id, &payload).await {
        Ok(resp) => {
            let id = extract_id(&resp, &["customerId", "id"]);
            if id != 0 {
                ctx.customers_by_name.insert(key, id);
                RowOutcome {
                    row_number,
                    status: RowStatus::Created,
                    label: name.to_string(),
                    message: format!("Created customer (ID: {})", id),
                }
            } else {
                emit_log(
                    app,
                    "warning",
                    &format!("Customer '{}' created but no ID returned", name),
                );
                RowOutcome {
                    row_number,
                    status: RowStatus::Created,
                    label: name.to_string(),
                    message: "Created (no ID returned)".into(),
                }
            }
        }
        Err(rest_err) => {
            // Try SOAP fallback
            if let Some(s) = soap {
                match s
                    .customer_add(
                        name,
                        ctx.source_so_id,
                        row.external_id.as_deref(),
                        row.contact_first_name.as_deref(),
                        row.contact_last_name.as_deref(),
                        row.contact_email.as_deref(),
                    )
                    .await
                {
                    Ok(id) if id > 0 => {
                        ctx.customers_by_name.insert(key, id);
                        return RowOutcome {
                            row_number,
                            status: RowStatus::Created,
                            label: name.to_string(),
                            message: format!("Created customer via SOAP (ID: {})", id),
                        };
                    }
                    Ok(_) => {}
                    Err(soap_err) => {
                        return RowOutcome {
                            row_number,
                            status: RowStatus::Error,
                            label: name.to_string(),
                            message: format!(
                                "Failed (REST: {}, SOAP: {})",
                                rest_err, soap_err
                            ),
                        };
                    }
                }
            }
            RowOutcome {
                row_number,
                status: RowStatus::Error,
                label: name.to_string(),
                message: format!("Failed: {}", rest_err),
            }
        }
    }
}

// ==================== Sites ====================

pub async fn import_site(
    row_number: usize,
    row: SiteImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    dry_run: bool,
    _app: &AppHandle,
) -> RowOutcome {
    let site_name = row.site_name.trim();
    let cust_name = row.customer_name.trim();
    let label = format!("{} / {}", cust_name, site_name);

    if site_name.is_empty() || cust_name.is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message: "customerName and siteName are required".into(),
        };
    }

    let cust_key = cust_name.to_lowercase();
    let site_key = site_name.to_lowercase();

    let cust_id = match ctx.customers_by_name.get(&cust_key) {
        Some(&id) => id,
        None => {
            return RowOutcome {
                row_number,
                status: RowStatus::Error,
                label,
                message: format!(
                    "Parent customer '{}' not found under SO {}",
                    cust_name, ctx.source_so_id
                ),
            }
        }
    };

    if let Some(&existing_id) = ctx.sites_by_name.get(&(cust_key.clone(), site_key.clone())) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label,
            message: format!("Site already exists (ID: {})", existing_id),
        };
    }

    if dry_run {
        return RowOutcome {
            row_number,
            status: RowStatus::Planned,
            label,
            message: format!("Would create site under customer ID {}", cust_id),
        };
    }

    let payload = json!({
        "siteName": site_name,
        "externalId": row.external_id,
        "contactFirstName": row.contact_first_name,
        "contactLastName": row.contact_last_name,
        "contactEmail": row.contact_email,
        "contactPhone": row.contact_phone,
        "street1": row.street1,
        "street2": row.street2,
        "city": row.city,
        "stateProv": row.state_prov,
        "country": row.country,
        "postalCode": row.postal_code,
    });

    match client.create_site(cust_id, &payload).await {
        Ok(resp) => {
            let id = extract_id(&resp, &["siteId", "id"]);
            if id != 0 {
                ctx.sites_by_name.insert((cust_key, site_key), id);
            }
            RowOutcome {
                row_number,
                status: RowStatus::Created,
                label,
                message: if id != 0 {
                    format!("Created site (ID: {})", id)
                } else {
                    "Created (no ID returned)".into()
                },
            }
        }
        Err(e) => RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message: format!("Failed: {}", e),
        },
    }
}

// ==================== Access Groups ====================

pub async fn import_access_group(
    row_number: usize,
    row: AccessGroupImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    soap: Option<&NcSoapClient>,
    dry_run: bool,
    _app: &AppHandle,
) -> RowOutcome {
    let name = row.group_name.trim();
    if name.is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: format!("(row {})", row_number),
            message: "groupName is required".into(),
        };
    }
    let key = name.to_lowercase();
    if let Some(&existing_id) = ctx.groups_by_name.get(&key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: name.to_string(),
            message: format!("Access group already exists (ID: {})", existing_id),
        };
    }

    // Determine placement OU
    let dest_ou = match row
        .customer_name
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(cn) => match ctx.customers_by_name.get(&cn.to_lowercase()) {
            Some(&id) => id,
            None => {
                return RowOutcome {
                    row_number,
                    status: RowStatus::Error,
                    label: name.to_string(),
                    message: format!("customerName '{}' not found", cn),
                }
            }
        },
        None => ctx.source_so_id,
    };

    // Resolve org unit scope from semicolon-separated names. Each entry can be a
    // customer name or a "Customer / Site" pair.
    let mut scope_ou_ids: Vec<String> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    if let Some(s) = row.org_unit_names.as_deref() {
        for entry in s.split(';').map(str::trim).filter(|e| !e.is_empty()) {
            let entry_lower = entry.to_lowercase();
            if let Some(&id) = ctx.customers_by_name.get(&entry_lower) {
                scope_ou_ids.push(id.to_string());
                continue;
            }
            // try "Customer/Site" or "Customer / Site"
            let parts: Vec<&str> = entry.split('/').map(str::trim).collect();
            if parts.len() == 2 {
                let key = (parts[0].to_lowercase(), parts[1].to_lowercase());
                if let Some(&site_id) = ctx.sites_by_name.get(&key) {
                    scope_ou_ids.push(site_id.to_string());
                    continue;
                }
            }
            unresolved.push(entry.to_string());
        }
    }
    // Default scope: if SO-level and nothing specified, all customers
    if scope_ou_ids.is_empty() && dest_ou == ctx.source_so_id {
        scope_ou_ids = ctx
            .customers_by_name
            .values()
            .map(|id| id.to_string())
            .collect();
    } else if scope_ou_ids.is_empty() {
        scope_ou_ids.push(dest_ou.to_string());
    }

    let group_type = row
        .group_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("ORG_UNIT");
    let description = row.group_description.as_deref().unwrap_or("");
    let auto_include = row
        .auto_include_new_org_units
        .as_deref()
        .unwrap_or("true");

    if dry_run {
        let extra = if unresolved.is_empty() {
            String::new()
        } else {
            format!(" (unresolved org units: {})", unresolved.join(", "))
        };
        return RowOutcome {
            row_number,
            status: RowStatus::Planned,
            label: name.to_string(),
            message: format!(
                "Would create {} access group at OU {} covering {} OUs{}",
                group_type,
                dest_ou,
                scope_ou_ids.len(),
                extra
            ),
        };
    }

    let empty_users: Vec<String> = Vec::new();
    let payload = json!({
        "groupName": name,
        "groupDescription": description,
        "orgUnitIds": scope_ou_ids,
        "userIds": empty_users,
        "autoIncludeNewOrgUnits": auto_include,
    });

    let result = if group_type == "DEVICE" {
        client.create_device_access_group(dest_ou, &payload).await
    } else {
        client.create_org_unit_access_group(dest_ou, &payload).await
    };

    match result {
        Ok(resp) => {
            let id = extract_id(&resp, &["groupId", "accessGroupId", "id"]);
            if id != 0 {
                ctx.groups_by_name.insert(key, id);
            }
            RowOutcome {
                row_number,
                status: RowStatus::Created,
                label: name.to_string(),
                message: if id != 0 {
                    format!("Created access group (ID: {})", id)
                } else {
                    "Created (no ID returned)".into()
                },
            }
        }
        Err(rest_err) => {
            if let Some(s) = soap {
                match s
                    .access_group_add(
                        name,
                        description,
                        dest_ou,
                        group_type,
                        auto_include == "true",
                    )
                    .await
                {
                    Ok(id) if id > 0 => {
                        ctx.groups_by_name.insert(key, id);
                        return RowOutcome {
                            row_number,
                            status: RowStatus::Created,
                            label: name.to_string(),
                            message: format!("Created access group via SOAP (ID: {})", id),
                        };
                    }
                    Ok(_) => {}
                    Err(soap_err) => {
                        return RowOutcome {
                            row_number,
                            status: RowStatus::Error,
                            label: name.to_string(),
                            message: format!(
                                "Failed (REST: {}, SOAP: {})",
                                rest_err, soap_err
                            ),
                        };
                    }
                }
            }
            RowOutcome {
                row_number,
                status: RowStatus::Error,
                label: name.to_string(),
                message: format!("Failed: {}", rest_err),
            }
        }
    }
}

// ==================== User Roles ====================

pub async fn import_user_role(
    row_number: usize,
    row: UserRoleImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    soap: Option<&NcSoapClient>,
    dry_run: bool,
    _app: &AppHandle,
) -> RowOutcome {
    let name = row.role_name.trim();
    if name.is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: format!("(row {})", row_number),
            message: "roleName is required".into(),
        };
    }
    let key = name.to_lowercase();
    if let Some(&existing_id) = ctx.roles_by_name.get(&key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: name.to_string(),
            message: format!("Role already exists (ID: {})", existing_id),
        };
    }

    let dest_ou = match row
        .customer_name
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(cn) => match ctx.customers_by_name.get(&cn.to_lowercase()) {
            Some(&id) => id,
            None => {
                return RowOutcome {
                    row_number,
                    status: RowStatus::Error,
                    label: name.to_string(),
                    message: format!("customerName '{}' not found", cn),
                }
            }
        },
        None => ctx.source_so_id,
    };

    // Resolve permission names -> ids
    let perm_names: Vec<String> = row
        .permissions
        .as_deref()
        .map(|s| {
            s.split(';')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut permission_ids = ctx.permission_lookup.names_to_ids(&perm_names);
    let unresolved: Vec<String> = perm_names
        .iter()
        .filter(|n| ctx.permission_lookup.get_id(n).is_none())
        .cloned()
        .collect();
    if permission_ids.is_empty() {
        // N-central rejects creation with zero permissions, so use a minimal fallback.
        permission_ids = vec![1701];
    }

    let description = row
        .role_description
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("Imported role");

    if dry_run {
        let extra = if unresolved.is_empty() {
            String::new()
        } else {
            format!(" (unresolved permissions: {})", unresolved.join(", "))
        };
        return RowOutcome {
            row_number,
            status: RowStatus::Planned,
            label: name.to_string(),
            message: format!(
                "Would create role at OU {} with {} permissions{}",
                dest_ou,
                permission_ids.len(),
                extra
            ),
        };
    }

    let empty_users: Vec<i64> = Vec::new();
    let payload = json!({
        "roleName": name,
        "description": description,
        "permissionIds": permission_ids,
        "userIds": empty_users,
    });

    // SOAP first for non-SO placement (REST always lands roles at the SO level)
    let is_so_level = dest_ou == ctx.source_so_id;
    if !is_so_level {
        if let Some(s) = soap {
            if let Ok(id) = s
                .user_role_add(name, description, dest_ou, &permission_ids)
                .await
            {
                if id > 0 {
                    ctx.roles_by_name.insert(key, id);
                    return RowOutcome {
                        row_number,
                        status: RowStatus::Created,
                        label: name.to_string(),
                        message: format!("Created role via SOAP at OU {} (ID: {})", dest_ou, id),
                    };
                }
            }
        }
    }

    match client.create_user_role(dest_ou, &payload).await {
        Ok(resp) => {
            let id = extract_id(&resp, &["roleId", "userRoleId", "id"]);
            if id != 0 {
                ctx.roles_by_name.insert(key, id);
            }
            RowOutcome {
                row_number,
                status: RowStatus::Created,
                label: name.to_string(),
                message: if id != 0 {
                    format!("Created role (ID: {})", id)
                } else {
                    "Created (no ID returned)".into()
                },
            }
        }
        Err(e) => RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: name.to_string(),
            message: format!("Failed: {}", e),
        },
    }
}

// ==================== Users ====================

pub async fn import_user(
    row_number: usize,
    row: UserImportRow,
    ctx: &mut ImportContext,
    soap: Option<&NcSoapClient>,
    dry_run: bool,
    _app: &AppHandle,
) -> RowOutcome {
    let login = row.login_name.trim();
    if login.is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: format!("(row {})", row_number),
            message: "loginName is required".into(),
        };
    }

    let cust_name = row.customer_name.trim();
    let dest_ou = if cust_name.eq_ignore_ascii_case("SO")
        || cust_name.eq_ignore_ascii_case("Service Org")
        || cust_name.is_empty()
    {
        ctx.source_so_id
    } else {
        match ctx.customers_by_name.get(&cust_name.to_lowercase()) {
            Some(&id) => id,
            None => {
                return RowOutcome {
                    row_number,
                    status: RowStatus::Error,
                    label: login.to_string(),
                    message: format!("customerName '{}' not found", cust_name),
                }
            }
        }
    };

    // Resolve role names
    let role_names: Vec<String> = row
        .role_names
        .as_deref()
        .map(|s| {
            s.split(';')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut role_ids: Vec<i64> = Vec::new();
    let mut unresolved_roles: Vec<String> = Vec::new();
    for n in &role_names {
        match ctx.roles_by_name.get(&n.to_lowercase()) {
            Some(&id) => role_ids.push(id),
            None => unresolved_roles.push(n.clone()),
        }
    }

    // Resolve access group names
    let group_names: Vec<String> = row
        .access_group_names
        .as_deref()
        .map(|s| {
            s.split(';')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut group_ids: Vec<i64> = Vec::new();
    let mut unresolved_groups: Vec<String> = Vec::new();
    for n in &group_names {
        match ctx.groups_by_name.get(&n.to_lowercase()) {
            Some(&id) => group_ids.push(id),
            None => unresolved_groups.push(n.clone()),
        }
    }

    if dry_run {
        let mut extras: Vec<String> = Vec::new();
        if !unresolved_roles.is_empty() {
            extras.push(format!("unresolved roles: {}", unresolved_roles.join(", ")));
        }
        if !unresolved_groups.is_empty() {
            extras.push(format!(
                "unresolved access groups: {}",
                unresolved_groups.join(", ")
            ));
        }
        let extra_str = if extras.is_empty() {
            String::new()
        } else {
            format!(" ({})", extras.join("; "))
        };
        return RowOutcome {
            row_number,
            status: RowStatus::Planned,
            label: login.to_string(),
            message: format!(
                "Would create user at OU {} with {} role(s), {} group(s){}",
                dest_ou,
                role_ids.len(),
                group_ids.len(),
                extra_str
            ),
        };
    }

    let soap = match soap {
        Some(s) => s,
        None => {
            return RowOutcome {
                row_number,
                status: RowStatus::Error,
                label: login.to_string(),
                message: "User creation requires SOAP client (not initialized)".into(),
            };
        }
    };

    let info = UserAddInfo {
        email: row.email.clone(),
        first_name: row.first_name.clone(),
        last_name: row.last_name.clone(),
        phone: row.phone.clone(),
        department: row.department.clone(),
        location: row.location.clone(),
        is_enabled: row.is_enabled,
        customer_id: dest_ou,
        role_ids: role_ids.clone(),
        access_group_ids: group_ids,
    };

    match soap.user_add(login, &info).await {
        Ok(id) if id > 0 => RowOutcome {
            row_number,
            status: RowStatus::Created,
            label: login.to_string(),
            message: format!("Created user (ID: {})", id),
        },
        Ok(id) => RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: login.to_string(),
            message: format!("SOAP returned non-positive ID: {}", id),
        },
        Err(e) => RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: login.to_string(),
            message: format!("Failed: {}", e),
        },
    }
}
