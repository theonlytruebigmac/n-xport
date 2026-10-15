//! Organization and device custom property value imports.

use super::common::emit_log;
use super::context::ImportContext;
use crate::api::NcClient;
use crate::import::{DevicePropertyImportRow, OrgPropertyImportRow, RowOutcome, RowStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Runtime};

pub(super) fn resolve_property_org_unit(
    row_number: usize,
    customer_name: Option<&str>,
    site_name: Option<&str>,
    ctx: &ImportContext,
) -> Result<(i64, String), RowOutcome> {
    let customer_name = customer_name.unwrap_or_default().trim();
    let site_name = site_name.unwrap_or_default().trim();
    if site_name.is_empty() && customer_name.is_empty() {
        return Ok((ctx.source_so_id, "Service Organization".to_string()));
    }
    if !site_name.is_empty() && customer_name.is_empty() {
        return Err(RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: site_name.to_string(),
            message: "customerName is required when siteName is provided".into(),
        });
    }
    let customer_id = ctx
        .customers_by_name
        .get(&customer_name.to_lowercase())
        .copied()
        .ok_or_else(|| RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: customer_name.to_string(),
            message: format!("Customer '{}' not found", customer_name),
        })?;
    if site_name.is_empty() {
        return Ok((customer_id, customer_name.to_string()));
    }
    let site_id = ctx
        .sites_by_name
        .get(&(customer_name.to_lowercase(), site_name.to_lowercase()))
        .copied()
        .ok_or_else(|| RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: format!("{} / {}", customer_name, site_name),
            message: format!(
                "Site '{}' was not found under customer '{}'",
                site_name, customer_name
            ),
        })?;
    Ok((site_id, format!("{} / {}", customer_name, site_name)))
}

pub(super) fn unique_property_id(
    property_name: &str,
    properties: impl IntoIterator<Item = (i64, Option<String>)>,
) -> Result<i64, String> {
    let property_name = property_name.trim();
    if property_name.is_empty() {
        return Err("propertyName is required".to_string());
    }
    let matches: Vec<i64> = properties
        .into_iter()
        .filter_map(|(id, name)| {
            name.filter(|name| name.trim().eq_ignore_ascii_case(property_name))
                .map(|_| id)
        })
        .collect();
    match matches.as_slice() {
        [id] => Ok(*id),
        [] => Err(format!(
            "Property '{}' was not found; property definitions must already exist in N-central (N-xport cannot create them)",
            property_name
        )),
        _ => Err(format!(
            "Property '{}' is ambiguous in this scope",
            property_name
        )),
    }
}

pub(super) fn validate_property_value(
    property_type: Option<&str>,
    choices: Option<&[String]>,
    value: &str,
) -> Result<(), String> {
    if property_type.is_some_and(|kind| kind.eq_ignore_ascii_case("ENUMERATED"))
        && !value.is_empty()
        && choices.is_some_and(|allowed| {
            !allowed
                .iter()
                .any(|choice| choice.eq_ignore_ascii_case(value))
        })
    {
        return Err(format!(
            "Value {:?} is not one of the enumerated property values",
            value
        ));
    }
    Ok(())
}

pub async fn import_org_property<R: Runtime>(
    row_number: usize,
    row: OrgPropertyImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    dry_run: bool,
    cancel_token: &AtomicBool,
    app: &AppHandle<R>,
) -> RowOutcome {
    let (org_unit_id, scope) = match resolve_property_org_unit(
        row_number,
        row.customer_name.as_deref(),
        row.site_name.as_deref(),
        ctx,
    ) {
        Ok(resolved) => resolved,
        Err(outcome) => return outcome,
    };
    let label = format!("{} / {}", scope, row.property_name.trim());
    if row.property_name.trim().is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message: "propertyName is required".into(),
        };
    }
    if let std::collections::hash_map::Entry::Vacant(e) =
        ctx.org_properties_by_ou.entry(org_unit_id)
    {
        match client.get_org_properties(org_unit_id).await {
            Ok(properties) => {
                e.insert(properties);
            }
            Err(error) => {
                return RowOutcome {
                    row_number,
                    status: RowStatus::Error,
                    label,
                    message: format!("Failed to fetch property definitions: {}", error),
                };
            }
        }
    }
    if cancel_token.load(Ordering::Relaxed) {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message: "Cancelled before property update".into(),
        };
    }
    let property_id = match unique_property_id(
        &row.property_name,
        ctx.org_properties_by_ou[&org_unit_id]
            .iter()
            .map(|property| (property.property_id, property.label.clone())),
    ) {
        Ok(id) => id,
        Err(message) => {
            return RowOutcome {
                row_number,
                status: RowStatus::Error,
                label,
                message,
            };
        }
    };
    let property = ctx.org_properties_by_ou[&org_unit_id]
        .iter()
        .find(|property| property.property_id == property_id)
        .expect("property ID was selected from the cached definitions");
    if let Err(message) = validate_property_value(
        property.property_type.as_deref(),
        property.enumerated_value_list.as_deref(),
        &row.value,
    ) {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message,
        };
    }
    if dry_run {
        return RowOutcome {
            row_number,
            status: RowStatus::Planned,
            label,
            message: format!("Would update property ID {} at {}", property_id, scope),
        };
    }
    match client
        .update_org_unit_property_value(org_unit_id, property_id, &row.value)
        .await
    {
        Ok(_) => RowOutcome {
            row_number,
            status: RowStatus::Updated,
            label,
            message: format!("Updated property value at {}", scope),
        },
        Err(error) => {
            let message = format!("Failed to update property value: {}", error);
            emit_log(app, "error", &message);
            RowOutcome {
                row_number,
                status: RowStatus::Error,
                label,
                message,
            }
        }
    }
}

pub async fn import_device_property<R: Runtime>(
    row_number: usize,
    row: DevicePropertyImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    dry_run: bool,
    cancel_token: &AtomicBool,
    app: &AppHandle<R>,
) -> RowOutcome {
    let customer_name = row.customer_name.trim();
    let site_name = row.site_name.as_deref().unwrap_or_default().trim();
    let device_name = row.device_name.trim();
    let label = format!(
        "{} / {} / {}",
        if site_name.is_empty() {
            customer_name.to_string()
        } else {
            format!("{} / {}", customer_name, site_name)
        },
        device_name,
        row.property_name.trim()
    );
    if customer_name.is_empty() || device_name.is_empty() || row.property_name.trim().is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message: "customerName, deviceName, and propertyName are required".into(),
        };
    }
    let (org_unit_id, scope) = match resolve_property_org_unit(
        row_number,
        Some(customer_name),
        row.site_name.as_deref(),
        ctx,
    ) {
        Ok(resolved) => resolved,
        Err(outcome) => return outcome,
    };
    if let std::collections::hash_map::Entry::Vacant(e) = ctx.devices_by_ou.entry(org_unit_id) {
        match client.get_devices_by_org_unit(org_unit_id).await {
            Ok(devices) => {
                e.insert(devices);
            }
            Err(error) => {
                return RowOutcome {
                    row_number,
                    status: RowStatus::Error,
                    label,
                    message: format!("Failed to fetch devices in {}: {}", scope, error),
                };
            }
        }
        if cancel_token.load(Ordering::Relaxed) {
            return RowOutcome {
                row_number,
                status: RowStatus::Error,
                label,
                message: "Cancelled before resolving device".into(),
            };
        }
    }
    let matching_devices: Vec<i64> = ctx.devices_by_ou[&org_unit_id]
        .iter()
        .filter(|device| {
            device
                .org_unit_scope_id()
                .is_none_or(|device_scope| device_scope == org_unit_id)
                && device
                    .long_name
                    .as_deref()
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case(device_name))
        })
        .map(|device| device.device_id)
        .collect();
    let device_id = match matching_devices.as_slice() {
        [device_id] => *device_id,
        [] => {
            return RowOutcome {
                row_number,
                status: RowStatus::Error,
                label,
                message: format!(
                    "Device '{}' was not found under {}; the device must already be on this server (N-xport does not create devices)",
                    device_name, scope
                ),
            };
        }
        _ => {
            return RowOutcome {
                row_number,
                status: RowStatus::Error,
                label,
                message: format!("Device '{}' is ambiguous under {}", device_name, scope),
            };
        }
    };
    if let std::collections::hash_map::Entry::Vacant(e) =
        ctx.device_properties_by_id.entry(device_id)
    {
        match client.get_device_properties(device_id).await {
            Ok(properties) => {
                e.insert(properties);
            }
            Err(error) => {
                return RowOutcome {
                    row_number,
                    status: RowStatus::Error,
                    label,
                    message: format!("Failed to fetch device property definitions: {}", error),
                };
            }
        }
    }
    if cancel_token.load(Ordering::Relaxed) {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message: "Cancelled before property update".into(),
        };
    }
    let property_id = match unique_property_id(
        &row.property_name,
        ctx.device_properties_by_id[&device_id]
            .iter()
            .map(|property| (property.property_id, property.label.clone())),
    ) {
        Ok(id) => id,
        Err(message) => {
            return RowOutcome {
                row_number,
                status: RowStatus::Error,
                label,
                message,
            };
        }
    };
    let property = ctx.device_properties_by_id[&device_id]
        .iter()
        .find(|property| property.property_id == property_id)
        .expect("property ID was selected from the cached definitions");
    if let Err(message) = validate_property_value(
        property.property_type.as_deref(),
        property.enumerated_value_list.as_deref(),
        &row.value,
    ) {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message,
        };
    }
    if dry_run {
        return RowOutcome {
            row_number,
            status: RowStatus::Planned,
            label,
            message: format!(
                "Would update device {} property ID {}",
                device_id, property_id
            ),
        };
    }
    match client
        .update_device_property_value(device_id, property_id, &row.value)
        .await
    {
        Ok(_) => RowOutcome {
            row_number,
            status: RowStatus::Updated,
            label,
            message: format!("Updated device property value (device ID {})", device_id),
        },
        Err(error) => {
            let message = format!("Failed to update device property value: {}", error);
            emit_log(app, "error", &message);
            RowOutcome {
                row_number,
                status: RowStatus::Error,
                label,
                message,
            }
        }
    }
}
