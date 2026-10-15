//! User role imports.

use super::common::extract_id;
use super::context::ImportContext;
use crate::api::{NcClient, NcSoapClient};
use crate::import::{RowOutcome, RowStatus, UserRoleImportRow};
use serde_json::json;
use tauri::{AppHandle, Runtime};

pub async fn import_user_role<R: Runtime>(
    row_number: usize,
    row: UserRoleImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    soap: Option<&NcSoapClient>,
    dry_run: bool,
    _app: &AppHandle<R>,
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

    // A same-named role at a sibling org unit is a different role; only one
    // defined here or inherited from the SO counts as already existing.
    if let Some(existing_id) = ctx.role_at(dest_ou, &key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: name.to_string(),
            message: format!(
                "Role already exists at or above OU {} (ID: {})",
                dest_ou, existing_id
            ),
        };
    }
    let unconfirmed_key = format!("role:{}:{}", dest_ou, key);
    if ctx.unconfirmed_at("role", dest_ou, &key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: name.to_string(),
            message: "Role was already created earlier in this session (server returned no ID)"
                .into(),
        };
    }

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

    let empty_users: Vec<String> = Vec::new();
    let permission_id_strings: Vec<String> =
        permission_ids.iter().map(ToString::to_string).collect();
    let payload = json!({
        "roleName": name,
        "description": description,
        "permissionIds": permission_id_strings,
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
                    ctx.record_role(dest_ou, key, id);
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
                ctx.record_role(dest_ou, key, id);
            } else {
                ctx.unconfirmed_creates.insert(unconfirmed_key);
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
