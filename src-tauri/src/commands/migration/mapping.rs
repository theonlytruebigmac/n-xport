//! Pure helpers that resolve source roles, groups, and property definitions to destination IDs.

use crate::models::*;
use std::collections::HashMap;

/// Resolve a source role to a destination role ID for a user at one destination
/// org unit. `visible_at_ou` is the lowercase-name -> ID map the destination
/// actually lists at that org unit (`None` when it could not be fetched). When
/// known, only roles visible there are returned, so a role ID that belongs to a
/// different customer is never assigned. When unknown, fall back to the direct
/// ID mapping and then the global name map.
pub(super) fn resolve_destination_role(
    source_role_id: i64,
    source_role_name: Option<&str>,
    direct: &HashMap<i64, i64>,
    global_names: &HashMap<String, i64>,
    visible_at_ou: Option<&HashMap<String, i64>>,
) -> Option<i64> {
    match visible_at_ou {
        Some(visible) => direct
            .get(&source_role_id)
            .copied()
            .filter(|id| visible.values().any(|visible_id| visible_id == id))
            .or_else(|| source_role_name.and_then(|name| visible.get(name).copied())),
        None => direct
            .get(&source_role_id)
            .copied()
            .or_else(|| source_role_name.and_then(|name| global_names.get(name).copied())),
    }
}

/// Map an access group's source org-unit scope to destination IDs. Returns the
/// mapped destination IDs and the source IDs that have no destination match.
pub(super) fn map_group_scope(
    source_org_unit_ids: &[i64],
    org_units: &HashMap<i64, i64>,
) -> (Vec<String>, Vec<i64>) {
    let mut mapped = Vec::new();
    let mut unmapped = Vec::new();
    for source_id in source_org_unit_ids {
        match org_units.get(source_id) {
            Some(dest_id) => mapped.push(dest_id.to_string()),
            None => unmapped.push(*source_id),
        }
    }
    (mapped, unmapped)
}

pub(super) fn destination_property_id(
    source_property: &OrgProperty,
    destination_properties: &[OrgProperty],
) -> Result<i64, String> {
    let label = source_property
        .label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .ok_or_else(|| "source property has no name".to_string())?;
    let matching_ids: Vec<i64> = destination_properties
        .iter()
        .filter(|property| {
            property
                .label
                .as_deref()
                .map(str::trim)
                .is_some_and(|destination_label| destination_label.eq_ignore_ascii_case(label))
        })
        .map(|property| property.property_id)
        .collect();

    match matching_ids.as_slice() {
        [property_id] => Ok(*property_id),
        [] => Err(format!("destination property {:?} was not found", label)),
        _ => Err(format!("destination property {:?} is ambiguous", label)),
    }
}

pub(super) fn destination_device_property_id(
    source_property: &DeviceProperty,
    destination_properties: &[DeviceProperty],
) -> Result<i64, String> {
    let label = source_property
        .label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .ok_or_else(|| "source device property has no name".to_string())?;
    let matching_ids: Vec<i64> = destination_properties
        .iter()
        .filter(|property| {
            property
                .label
                .as_deref()
                .map(str::trim)
                .is_some_and(|destination_label| destination_label.eq_ignore_ascii_case(label))
        })
        .map(|property| property.property_id)
        .collect();

    match matching_ids.as_slice() {
        [property_id] => Ok(*property_id),
        [] => Err(format!("destination property {:?} was not found", label)),
        _ => Err(format!("destination property {:?} is ambiguous", label)),
    }
}
