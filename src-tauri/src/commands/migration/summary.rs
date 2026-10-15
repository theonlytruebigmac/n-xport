//! Per-category outcomes collected during a run, and the final result built from them.

use super::types::{MigrationOptions, MigrationResult};

/// Collects per-category outcomes for the post-migration summary report.
#[derive(Default)]
pub(super) struct MigrationSummary {
    pub(super) customers_failed: Vec<String>,
    pub(super) sites_failed: Vec<String>,
    pub(super) roles_failed: Vec<String>,
    /// Users created without any role because their source role(s) could not be mapped
    /// (e.g. system/built-in roles are not returned by the custom roles API).
    pub(super) users_no_roles: Vec<(String, Vec<String>)>, // (login, source_role_ids as strings)
    pub(super) users_failed: Vec<String>,
    pub(super) properties_failed: Vec<String>,
    pub(super) properties_skipped: Vec<String>,
    pub(super) fetch_failures: Vec<String>,
    pub(super) skipped: Vec<String>,
    /// Access groups successfully created on the destination (users included in payload).
    pub(super) access_groups_created: Vec<String>,
    /// Access groups that already existed on the destination.
    /// N-central has no API to update group membership, so newly migrated users
    /// from the source were NOT added to these groups.
    pub(super) access_groups_existed_not_updated: Vec<String>,
    pub(super) access_groups_failed: Vec<String>,
    /// Access groups created by the SOAP fallback, which cannot set members or
    /// org-unit scope.
    pub(super) access_groups_created_via_soap: Vec<String>,
    pub(super) additional_manual_follow_up: Vec<String>,
}

impl MigrationSummary {
    pub(super) fn issues(&self) -> Vec<String> {
        self.customers_failed
            .iter()
            .chain(&self.sites_failed)
            .chain(&self.roles_failed)
            .chain(&self.users_failed)
            .chain(&self.access_groups_failed)
            .chain(&self.properties_failed)
            .chain(&self.fetch_failures)
            .cloned()
            .collect()
    }

    /// Groups created by a run that did not migrate users have no members, and
    /// membership can only be set when a group is created.
    pub(super) fn note_memberless_access_groups(&mut self, options: &MigrationOptions) {
        if options.access_groups && !options.users && !self.access_groups_created.is_empty() {
            self.additional_manual_follow_up.push(format!(
                "{} access group(s) were created without members because Users was not selected: {}. \
                 Add members manually.",
                self.access_groups_created.len(),
                self.access_groups_created.join(", ")
            ));
        }
    }

    pub(super) fn manual_follow_up(&self) -> Vec<String> {
        self.users_no_roles
            .iter()
            .map(|(login, roles)| {
                format!(
                    "User '{}' was created without these source roles: {}",
                    login,
                    roles.join(", ")
                )
            })
            .chain(self.access_groups_existed_not_updated.iter().map(|group| {
                format!(
                    "Existing access group {}; it was left unchanged because N-central cannot add users to an existing group",
                    group
                )
            }))
            .chain(self.access_groups_created_via_soap.iter().map(|group| {
                format!(
                    "Access group '{}' was created via SOAP without its members or org-unit scope; add them manually",
                    group
                )
            }))
            .chain(self.additional_manual_follow_up.iter().cloned())
            .chain(self.properties_skipped.iter().cloned())
            .collect()
    }
}

pub(super) fn build_migration_result(
    summary: &MigrationSummary,
    cancelled: bool,
) -> MigrationResult {
    let issues = summary.issues();
    let manual_follow_up = summary.manual_follow_up();
    let has_issues = !issues.is_empty() || !manual_follow_up.is_empty();
    let message = if cancelled {
        "Migration cancelled; completed changes were kept".to_string()
    } else if has_issues {
        format!(
            "Migration completed with {} failure(s) and {} manual follow-up item(s)",
            issues.len(),
            manual_follow_up.len()
        )
    } else {
        "Migration completed successfully".to_string()
    };

    MigrationResult {
        success: !cancelled && !has_issues,
        cancelled,
        issues,
        skipped: summary.skipped.clone(),
        manual_follow_up,
        message,
    }
}
