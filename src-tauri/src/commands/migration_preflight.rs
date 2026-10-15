//! Preflight summary for a migration: what will run, in what order, where SOAP
//! may be used, and which follow-up work is already known before starting.

use serde::Serialize;

use super::migration::MigrationOptions;

/// How a migration step uses the SOAP API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SoapUse {
    /// REST only.
    None,
    /// REST first; SOAP is attempted only if the REST call fails.
    Fallback,
    /// SOAP is tried first and REST is used if SOAP is unavailable.
    Preferred,
    /// The step cannot do its work without SOAP.
    Required,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightStep {
    pub resource: String,
    pub label: String,
    pub soap: SoapUse,
    pub details: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreflight {
    pub source_so_id: i64,
    pub dest_so_id: i64,
    /// Steps in execution order.
    pub steps: Vec<PreflightStep>,
    /// True when any step may call SOAP (fallback or required).
    pub uses_soap: bool,
    /// Things that must already be true on the destination before the run starts.
    /// Unlike follow-up, these are actions to take first.
    pub prerequisites: Vec<String>,
    /// Problems with the selection itself (for example, nothing selected).
    pub warnings: Vec<String>,
    /// Manual work and limitations that are already known before the run.
    pub manual_follow_up: Vec<String>,
}

fn step(resource: &str, label: &str, soap: SoapUse, details: &str) -> PreflightStep {
    PreflightStep {
        resource: resource.into(),
        label: label.into(),
        soap,
        details: details.into(),
    }
}

/// Describe a migration without contacting either server. Step order mirrors
/// `start_migration`.
pub fn build_preflight(
    options: &MigrationOptions,
    source_so_id: i64,
    dest_so_id: i64,
) -> MigrationPreflight {
    let mut steps = Vec::new();
    let mut warnings = Vec::new();
    let mut prerequisites = Vec::new();
    let mut manual_follow_up = Vec::new();

    if options.customers {
        steps.push(step(
            "customers",
            "Customers and sites",
            SoapUse::Fallback,
            "Created over REST; SOAP customerAdd is tried only if a REST create fails. \
             Customers and sites already on the destination (matched by name) are skipped.",
        ));
    } else if options.needs_existing_org_unit_mapping() {
        steps.push(step(
            "org_unit_mapping",
            "Match existing customers and sites",
            SoapUse::None,
            "Read-only. Customers and sites are matched to the destination by unique name.",
        ));
        manual_follow_up.push(
            "Customers is not selected: customers and sites missing from the destination, or \
             matching more than one destination entry, are skipped along with the roles, users, \
             access groups, and properties beneath them."
                .into(),
        );
    }
    if options.user_roles {
        steps.push(step(
            "user_roles",
            "User roles",
            SoapUse::Preferred,
            "Service-org roles are created over REST. Customer- and site-level roles are created \
             with SOAP userRoleAdd when it is available, and over REST otherwise. On N-central \
             2026.4 REST places customer-level roles correctly (checked on a live server); \
             older servers were reported to place them at the service-org level, so check where \
             the roles landed if SOAP is unavailable.",
        ));
    }
    if options.users {
        steps.push(step(
            "users",
            "Users",
            SoapUse::Required,
            "User creation exists only in SOAP (userAdd). Users already on the destination are skipped.",
        ));
        manual_follow_up.push(
            "Users are created with generated passwords that are not recorded; have them reset \
             their passwords."
                .into(),
        );
        manual_follow_up.push(
            "Users whose source role is a system role unavailable through the API, or is not \
             available at their destination org unit, are created without that role; assign it \
             manually."
                .into(),
        );
    }
    if options.access_groups {
        steps.push(step(
            "access_groups",
            "Access groups",
            SoapUse::Fallback,
            "Created over REST with their members; SOAP is tried only if a REST create fails. \
             Runs after users so their destination IDs can be included.",
        ));
        manual_follow_up.push(
            "Access groups that already exist on the destination are left unchanged. N-central \
             has no API to change the membership of an existing group, so migrated users are not \
             added to them."
                .into(),
        );
        manual_follow_up.push(
            "Device-based access groups are not migrated because device ID mapping is not \
             implemented."
                .into(),
        );
        manual_follow_up.push(
            "A group whose org units are only partly present on the destination is created for \
             the matching org units only, and one with none is not created. A group created by the \
             SOAP fallback has no members or org-unit scope and must be completed manually."
                .into(),
        );
        if !options.users {
            manual_follow_up.push(
                "Users is not selected: new access groups are created without the source \
                 group's members, and membership cannot be added afterwards by this tool."
                    .into(),
            );
        }
    }
    if options.org_properties {
        steps.push(step(
            "org_properties",
            "Organization properties",
            SoapUse::Fallback,
            "Values are written over REST to existing destination definitions matched by name; \
             SOAP is tried only if the REST write fails. Definitions are not created.",
        ));
        prerequisites.push(
            "Organization custom property definitions must already exist on the destination. \
             N-xport migrates values only and cannot create definitions; create each one in \
             N-central first, or its values are reported and skipped."
                .into(),
        );
    }
    if options.device_properties {
        steps.push(step(
            "device_properties",
            "Device properties",
            SoapUse::None,
            "REST only. Only devices already on the destination, matched by unique name within \
             the mapped org unit, are updated; no devices are created or moved.",
        ));
        prerequisites.push(
            "The devices must already be on the destination. N-xport does not move or create \
             devices; install or register each device's agent against the destination server \
             first. Devices that are not there yet are reported and skipped."
                .into(),
        );
        prerequisites.push(
            "Device custom property definitions must already exist on the destination, as for \
             organization properties."
                .into(),
        );
    }

    if steps.is_empty() {
        warnings.push("No data is selected, so nothing will be migrated.".into());
    }
    let uses_soap = steps.iter().any(|step| step.soap != SoapUse::None);

    MigrationPreflight {
        source_so_id,
        dest_so_id,
        steps,
        uses_soap,
        prerequisites,
        warnings,
        manual_follow_up,
    }
}

/// Describe a migration before it starts. Nothing is read from or written to
/// either server.
#[tauri::command]
pub fn migration_preflight(
    options: MigrationOptions,
    source_so_id: i64,
    dest_so_id: i64,
) -> MigrationPreflight {
    build_preflight(&options, source_so_id, dest_so_id)
}

#[cfg(test)]
mod tests;
