use crate::commands::migration::summary::{build_migration_result, MigrationSummary};
use crate::commands::migration::types::MigrationOptions;

fn options(users: bool, groups: bool) -> MigrationOptions {
    MigrationOptions {
        customers: true,
        user_roles: false,
        users,
        access_groups: groups,
        org_properties: false,
        device_properties: false,
    }
}

fn summary_with_new_group() -> MigrationSummary {
    MigrationSummary {
        access_groups_created: vec!["Helpdesk (at Customer 10)".into()],
        ..Default::default()
    }
}

#[test]
fn groups_created_without_migrating_users_are_flagged_as_memberless() {
    let mut summary = summary_with_new_group();
    summary.note_memberless_access_groups(&options(false, true));
    let result = build_migration_result(&summary, false);
    assert!(!result.success, "memberless groups need manual follow-up");
    assert!(
        result
            .manual_follow_up
            .iter()
            .any(|item| item.contains("without members")
                && item.contains("Helpdesk (at Customer 10)")),
        "{:?}",
        result.manual_follow_up
    );
}

#[test]
fn no_note_when_users_migrated_with_the_groups_or_no_group_was_created() {
    let mut summary = summary_with_new_group();
    summary.note_memberless_access_groups(&options(true, true));
    assert!(summary.manual_follow_up().is_empty());

    let mut summary = MigrationSummary::default();
    summary.note_memberless_access_groups(&options(false, true));
    assert!(summary.manual_follow_up().is_empty());
}

#[test]
fn existing_groups_are_reported_as_unchanged() {
    let summary = MigrationSummary {
        access_groups_existed_not_updated: vec![
            "'Helpdesk' (at Customer 10) is missing 1 migrated user(s)".into(),
        ],
        ..Default::default()
    };
    let follow_up = summary.manual_follow_up().join("\n");
    assert!(follow_up.contains("left unchanged"), "{}", follow_up);
    assert!(
        follow_up.contains("'Helpdesk' (at Customer 10)"),
        "{}",
        follow_up
    );
}
