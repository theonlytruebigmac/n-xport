//! User imports (SOAP only).

use super::context::ImportContext;
use crate::api::{NcSoapClient, UserAddInfo};
use crate::config::PasswordPolicy;
use crate::import::{RowOutcome, RowStatus, UserImportRow};
use tauri::{AppHandle, Runtime};

pub async fn import_user<R: Runtime>(
    row_number: usize,
    row: UserImportRow,
    ctx: &mut ImportContext,
    soap: Option<&NcSoapClient>,
    dry_run: bool,
    password_policy: Option<&PasswordPolicy>,
    _app: &AppHandle<R>,
) -> RowOutcome {
    // N-central uses the email as the login name. The CSV intentionally has no
    // separate loginName column — `email` is both.
    let login = row.email.trim();
    if login.is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: format!("(row {})", row_number),
            message: "email is required".into(),
        };
    }

    // Resolve the customerName column against the connected SO (recommended),
    // a customer under it, or a site. Sentinels "SO" / "Service Org" and an
    // empty value also place the user at SO level.
    let cust_name = row.customer_name.trim();
    let cust_key = cust_name.to_lowercase();
    let (dest_ou, scope_label) = if cust_name.is_empty()
        || cust_name.eq_ignore_ascii_case("SO")
        || cust_name.eq_ignore_ascii_case("Service Org")
        || ctx
            .source_so_name
            .as_deref()
            .map(|n| n == cust_key)
            .unwrap_or(false)
    {
        (ctx.source_so_id, "SO".to_string())
    } else if let Some(&id) = ctx.customers_by_name.get(&cust_key) {
        (id, format!("customer '{}'", cust_name))
    } else if let Some(&id) = ctx.sites_by_name_flat.get(&cust_key) {
        (id, format!("site '{}'", cust_name))
    } else {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: login.to_string(),
            message: format!(
                "customerName '{}' did not match any service org, customer, or site under the connected SO",
                cust_name
            ),
        };
    };

    let created_key = format!("{}@{}", login.to_lowercase(), dest_ou);
    if ctx.created_users.contains(&created_key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: login.to_string(),
            message: format!(
                "User was already created earlier in this session at {} (OU {})",
                scope_label, dest_ou
            ),
        };
    }

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
        // Resolve against what the user's own org unit can see; only org units
        // that were not loaded (sites) fall back to the global first-seen map.
        let found = if ctx.roles_by_ou.contains_key(&dest_ou) {
            ctx.role_at(dest_ou, &n.to_lowercase())
        } else {
            ctx.roles_by_name.get(&n.to_lowercase()).copied()
        };
        match found {
            Some(id) => role_ids.push(id),
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
        let found = if ctx.groups_by_ou.contains_key(&dest_ou) {
            ctx.group_at(dest_ou, &n.to_lowercase())
        } else {
            ctx.groups_by_name.get(&n.to_lowercase()).copied()
        };
        match found {
            Some(id) => group_ids.push(id),
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
                "Would create user at {} (OU {}) with {} role(s), {} group(s){}",
                scope_label,
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

    match soap.user_add(login, &info, password_policy).await {
        Ok(id) if id > 0 => {
            ctx.created_users.insert(created_key);
            RowOutcome {
                row_number,
                status: RowStatus::Created,
                label: login.to_string(),
                message: format!("Created user (ID: {}) at {} (OU {})", id, scope_label, dest_ou),
            }
        }
        // N-central's userAdd returns -1 (with no SOAP fault) when it refuses
        // to create the user — the most common cause is that a user with this
        // login already exists at or above the target OU. Surface as Skipped
        // so re-running the same CSV doesn't tally these as errors.
        Ok(id) => RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: login.to_string(),
            message: format!(
                "User not created (server returned ID {}); likely already exists at or above {} (OU {})",
                id, scope_label, dest_ou
            ),
        },
        Err(e) => {
            let msg = e.to_string();
            // If a SOAP fault explicitly mentions duplication, also classify as Skipped.
            let is_duplicate = msg.to_lowercase().contains("already exists")
                || msg.to_lowercase().contains("duplicate");
            RowOutcome {
                row_number,
                status: if is_duplicate { RowStatus::Skipped } else { RowStatus::Error },
                label: login.to_string(),
                message: if is_duplicate {
                    format!("User already exists: {}", msg)
                } else {
                    format!("Failed: {}", msg)
                },
            }
        }
    }
}
