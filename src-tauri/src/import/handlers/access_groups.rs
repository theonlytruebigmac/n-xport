//! Access group imports.

use super::common::extract_id;
use super::context::ImportContext;
use crate::api::{NcClient, NcSoapClient};
use crate::import::{AccessGroupImportRow, RowOutcome, RowStatus};
use serde_json::json;
use tauri::{AppHandle, Runtime};

pub(super) fn validate_import_group_type(group_type: &str) -> Result<(), String> {
    if group_type.eq_ignore_ascii_case("ORG_UNIT") {
        Ok(())
    } else if group_type.eq_ignore_ascii_case("DEVICE") {
        Err("Device-based access-group import is unsupported because device ID mapping is not implemented".into())
    } else {
        Err(format!(
            "Unsupported groupType '{}'; expected ORG_UNIT",
            group_type
        ))
    }
}

pub async fn import_access_group<R: Runtime>(
    row_number: usize,
    row: AccessGroupImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    soap: Option<&NcSoapClient>,
    dry_run: bool,
    _app: &AppHandle<R>,
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

    // A same-named group at a sibling org unit is a different group; only one
    // defined here or inherited from the SO counts as already existing.
    if let Some(existing_id) = ctx.group_at(dest_ou, &key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: name.to_string(),
            message: format!(
                "Access group already exists at or above OU {} (ID: {})",
                dest_ou, existing_id
            ),
        };
    }
    let unconfirmed_key = format!("group:{}:{}", dest_ou, key);
    if ctx.unconfirmed_at("group", dest_ou, &key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: name.to_string(),
            message:
                "Access group was already created earlier in this session (server returned no ID)"
                    .into(),
        };
    }

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
    if let Err(message) = validate_import_group_type(group_type) {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: name.to_string(),
            message,
        };
    }
    let description = row.group_description.as_deref().unwrap_or("");
    let auto_include = row.auto_include_new_org_units.as_deref().unwrap_or("true");

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

    let result = client.create_org_unit_access_group(dest_ou, &payload).await;

    match result {
        Ok(resp) => {
            let mut id = extract_id(&resp, &["groupId", "accessGroupId", "id"]);
            if id == 0 {
                // Creating an access group returns no body; find the new group's ID by name.
                id = client
                    .find_access_group_id(dest_ou, name)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(0);
            }
            if id != 0 {
                ctx.record_group(dest_ou, key, id);
            } else {
                ctx.unconfirmed_creates.insert(unconfirmed_key);
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
                        ctx.record_group(dest_ou, key, id);
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
                            message: format!("Failed (REST: {}, SOAP: {})", rest_err, soap_err),
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
