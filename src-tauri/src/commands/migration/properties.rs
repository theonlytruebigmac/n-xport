//! Organization and device custom property values.

use super::env::{emit_log, fetch_or_warn, report_progress, MigrationEnv};
use super::mapping::{destination_device_property_id, destination_property_id};
use super::summary::MigrationSummary;
use super::types::IdMapping;
use crate::models::*;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::Runtime;

/// Migrate organization custom properties from source to destination.
pub(super) async fn migrate_org_properties<R: Runtime>(
    env: &MigrationEnv<'_, R>,
    mapping: &IdMapping,
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
    report_progress(
        app_handle,
        "Properties",
        "Migrating org custom properties...",
        90.0,
    );

    // Property values are listed per org unit and carry no org-unit ID of their own, so
    // walk the mapped org units top-down (service org, customers, sites). A value a lower
    // org unit merely inherits is already the same on the destination once its parent has
    // been written, so only values that differ from what the destination shows are written.
    let mut pairs: Vec<(i64, i64)> = vec![(source_so_id, dest_so_id)];
    let mut customers: Vec<(i64, i64)> = mapping.customers.iter().map(|(&s, &d)| (s, d)).collect();
    customers.sort_unstable();
    let mut sites: Vec<(i64, i64)> = mapping.sites.iter().map(|(&s, &d)| (s, d)).collect();
    sites.sort_unstable();
    pairs.extend(customers);
    pairs.extend(sites);

    let mut synced = 0;
    let mut unchanged = 0;
    let mut failed = 0;
    let mut reported_missing: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (src_ou, dest_ou) in pairs {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        let failures_before = summary.fetch_failures.len();
        let source_props = fetch_or_warn(
            source.get_org_properties(src_ou).await,
            app_handle,
            &format!("source properties for org unit {}", src_ou),
            &mut summary.fetch_failures,
        );
        let dest_props = fetch_or_warn(
            dest.get_org_properties(dest_ou).await,
            app_handle,
            &format!("destination properties for org unit {}", dest_ou),
            &mut summary.fetch_failures,
        );
        if summary.fetch_failures.len() != failures_before {
            failed += 1;
            continue;
        }

        for prop in &source_props {
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }
            let dest_property_id = match destination_property_id(prop, &dest_props) {
                Ok(property_id) => property_id,
                Err(reason) => {
                    // Definitions cannot be created through the API, and the same missing
                    // definition shows up at every org unit, so say it once.
                    if reported_missing.insert(format!("{:?}", prop.label)) {
                        let hint = if reason.contains("not found") {
                            ". Property definitions must already exist on the destination \
                             (N-xport cannot create them): create it there, then run the migration again"
                        } else {
                            ""
                        };
                        let message = format!(
                            "Property {:?} was not synced: {}{}",
                            prop.label, reason, hint
                        );
                        emit_log(app_handle, "warning", &message);
                        summary.properties_skipped.push(message.clone());
                        summary.skipped.push(message);
                    }
                    continue;
                }
            };

            let value = prop.value.as_deref().unwrap_or("");
            let current = dest_props
                .iter()
                .find(|candidate| candidate.property_id == dest_property_id)
                .and_then(|candidate| candidate.value.as_deref())
                .unwrap_or("");
            if current == value {
                unchanged += 1;
                continue;
            }

            let update_result = dest
                .update_org_unit_property_value(dest_ou, dest_property_id, value)
                .await;
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }
            match update_result {
                Ok(_) => {
                    synced += 1;
                    emit_log(
                        app_handle,
                        "debug",
                        &format!("Synced property {:?} to OU {}", prop.label, dest_ou),
                    );
                }
                Err(rest_err) => {
                    // REST failed — try SOAP fallback
                    if let Some(soap) = soap_client {
                        match soap
                            .organization_property_modify(dest_ou, dest_property_id, value)
                            .await
                        {
                            Ok(_) => {
                                synced += 1;
                                emit_log(
                                    app_handle,
                                    "debug",
                                    &format!(
                                        "Synced property {:?} to OU {} via SOAP fallback",
                                        prop.label, dest_ou
                                    ),
                                );
                            }
                            Err(soap_err) => {
                                let msg = format!(
                                    "Failed to sync property {:?} to OU {} (REST: {}, SOAP: {})",
                                    prop.label, dest_ou, rest_err, soap_err
                                );
                                tracing::warn!("{}", msg);
                                emit_log(app_handle, "debug", &msg);
                                summary.properties_failed.push(msg);
                                failed += 1;
                            }
                        }
                    } else {
                        let msg = format!(
                            "Failed to sync property {:?} to OU {}: {}",
                            prop.label, dest_ou, rest_err
                        );
                        tracing::warn!("{}", msg);
                        emit_log(app_handle, "debug", &msg);
                        summary.properties_failed.push(msg);
                        failed += 1;
                    }
                }
            }
        }
    }

    emit_log(
        app_handle,
        "info",
        &format!(
            "Properties: {} synced, {} already matched, {} failed",
            synced, unchanged, failed
        ),
    );

    Ok(())
}

pub(super) async fn migrate_device_properties<R: Runtime>(
    env: &MigrationEnv<'_, R>,
    mapping: &IdMapping,
    summary: &mut MigrationSummary,
) -> Result<(), String> {
    let MigrationEnv {
        source,
        dest,
        app_handle,
        cancel_token,
        ..
    } = *env;
    if cancel_token.load(Ordering::Relaxed) {
        return Ok(());
    }
    report_progress(
        app_handle,
        "Device Properties",
        "Migrating device custom properties...",
        94.0,
    );

    let mut seen_source_devices = std::collections::HashSet::new();
    let mut devices_not_on_destination = 0usize;
    let mut destination_devices_by_ou: HashMap<i64, Vec<Device>> = HashMap::new();
    let mut destination_properties_by_device: HashMap<i64, Vec<DeviceProperty>> = HashMap::new();

    for (&source_ou_id, &dest_ou_id) in &mapping.org_units {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        let source_devices = match source.get_devices_by_org_unit(source_ou_id).await {
            Ok(devices) => devices,
            Err(error) => {
                let message = format!(
                    "Failed to fetch source devices for OU {}: {}",
                    source_ou_id, error
                );
                tracing::warn!("{}", message);
                summary.fetch_failures.push(message.clone());
                emit_log(app_handle, "warning", &message);
                continue;
            }
        };
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }

        if let std::collections::hash_map::Entry::Vacant(e) =
            destination_devices_by_ou.entry(dest_ou_id)
        {
            match dest.get_devices_by_org_unit(dest_ou_id).await {
                Ok(devices) => {
                    e.insert(devices);
                }
                Err(error) => {
                    let message = format!(
                        "Failed to fetch destination devices for OU {}: {}",
                        dest_ou_id, error
                    );
                    tracing::warn!("{}", message);
                    summary.fetch_failures.push(message.clone());
                    emit_log(app_handle, "warning", &message);
                    continue;
                }
            }
        }
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }

        for source_device in source_devices {
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }
            let source_device_ou = match source_device.org_unit_scope_id() {
                Some(org_unit_id) => org_unit_id,
                None => {
                    summary.additional_manual_follow_up.push(format!(
                        "Device ID {} has no org-unit scope; its custom properties were not migrated",
                        source_device.device_id
                    ));
                    continue;
                }
            };
            let mapped_dest_ou = match mapping.org_units.get(&source_device_ou) {
                Some(org_unit_id) => *org_unit_id,
                None => {
                    summary.additional_manual_follow_up.push(format!(
                        "Device ID {} belongs to unmapped source OU {}; its custom properties were not migrated",
                        source_device.device_id, source_device_ou
                    ));
                    continue;
                }
            };
            if mapped_dest_ou != dest_ou_id {
                continue;
            }
            if !seen_source_devices.insert(source_device.device_id) {
                continue;
            }
            let device_name = match source_device
                .long_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                Some(name) => name,
                None => {
                    summary.additional_manual_follow_up.push(format!(
                        "Device ID {} has no long name; its custom properties were not migrated",
                        source_device.device_id
                    ));
                    continue;
                }
            };
            let destination_matches: Vec<&Device> = destination_devices_by_ou[&dest_ou_id]
                .iter()
                .filter(|device| {
                    device.org_unit_scope_id() == Some(dest_ou_id)
                        && device
                            .long_name
                            .as_deref()
                            .is_some_and(|name| name.trim().eq_ignore_ascii_case(device_name))
                })
                .collect();
            let destination_device = match destination_matches.as_slice() {
                [device] => *device,
                [] => {
                    summary.additional_manual_follow_up.push(format!(
                        "Device '{}' was not found in destination OU {}; its custom properties were not migrated",
                        device_name, dest_ou_id
                    ));
                    devices_not_on_destination += 1;
                    continue;
                }
                _ => {
                    summary.additional_manual_follow_up.push(format!(
                        "Device '{}' is ambiguous in destination OU {}; its custom properties were not migrated",
                        device_name, dest_ou_id
                    ));
                    continue;
                }
            };

            let source_properties =
                match source.get_device_properties(source_device.device_id).await {
                    Ok(properties) => properties,
                    Err(error) => {
                        let message = format!(
                            "Failed to fetch custom properties for source device '{}': {}",
                            device_name, error
                        );
                        tracing::warn!("{}", message);
                        summary.fetch_failures.push(message.clone());
                        emit_log(app_handle, "warning", &message);
                        continue;
                    }
                };
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }

            if let std::collections::hash_map::Entry::Vacant(e) =
                destination_properties_by_device.entry(destination_device.device_id)
            {
                match dest
                    .get_device_properties(destination_device.device_id)
                    .await
                {
                    Ok(properties) => {
                        e.insert(properties);
                    }
                    Err(error) => {
                        let message = format!(
                            "Failed to fetch custom-property definitions for destination device '{}': {}",
                            device_name, error
                        );
                        tracing::warn!("{}", message);
                        summary.fetch_failures.push(message.clone());
                        emit_log(app_handle, "warning", &message);
                        continue;
                    }
                }
            }
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }

            for property in source_properties {
                if cancel_token.load(Ordering::Relaxed) {
                    break;
                }
                let dest_property_id = match destination_device_property_id(
                    &property,
                    &destination_properties_by_device[&destination_device.device_id],
                ) {
                    Ok(property_id) => property_id,
                    Err(reason) => {
                        summary.additional_manual_follow_up.push(format!(
                            "Device '{}' property {:?} was not migrated: {}. Device property definitions must already exist on the destination (N-xport cannot create them)",
                            device_name, property.label, reason
                        ));
                        continue;
                    }
                };
                let value = property.value.as_deref().unwrap_or("");
                match dest
                    .update_device_property_value(
                        destination_device.device_id,
                        dest_property_id,
                        value,
                    )
                    .await
                {
                    Ok(_) => {
                        emit_log(
                            app_handle,
                            "success",
                            &format!(
                                "Migrated device property '{}' for '{}'",
                                property.label.as_deref().unwrap_or("(unnamed)"),
                                device_name
                            ),
                        );
                    }
                    Err(error) => {
                        let message = format!(
                            "Failed to migrate device property '{}' for '{}': {}",
                            property.label.as_deref().unwrap_or("(unnamed)"),
                            device_name,
                            error
                        );
                        tracing::warn!("{}", message);
                        summary.properties_failed.push(message.clone());
                        emit_log(app_handle, "error", &message);
                    }
                }
            }
        }
    }

    if devices_not_on_destination > 0 {
        let message = format!(
            "{} device(s) with custom properties are not on the destination yet. N-xport does not move devices: install or register their agents against the destination server, then run the device-property migration again",
            devices_not_on_destination
        );
        emit_log(app_handle, "warning", &message);
        summary.additional_manual_follow_up.push(message);
    }
    Ok(())
}
