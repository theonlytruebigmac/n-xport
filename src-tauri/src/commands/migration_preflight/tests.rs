use super::*;

fn options(
    customers: bool,
    roles: bool,
    users: bool,
    groups: bool,
    props: bool,
    devices: bool,
) -> MigrationOptions {
    MigrationOptions {
        customers,
        user_roles: roles,
        users,
        access_groups: groups,
        org_properties: props,
        device_properties: devices,
    }
}

fn resources(preflight: &MigrationPreflight) -> Vec<&str> {
    preflight
        .steps
        .iter()
        .map(|step| step.resource.as_str())
        .collect()
}

fn soap_for(preflight: &MigrationPreflight, resource: &str) -> SoapUse {
    preflight
        .steps
        .iter()
        .find(|step| step.resource == resource)
        .unwrap_or_else(|| panic!("no {} step", resource))
        .soap
}

#[test]
fn steps_follow_the_order_migration_runs_them() {
    let preflight = build_preflight(&options(true, true, true, true, true, true), 1, 2);
    assert_eq!(
        resources(&preflight),
        vec![
            "customers",
            "user_roles",
            "users",
            "access_groups",
            "org_properties",
            "device_properties"
        ]
    );
    assert_eq!((preflight.source_so_id, preflight.dest_so_id), (1, 2));
}

#[test]
fn soap_use_is_reported_per_step() {
    let preflight = build_preflight(&options(true, true, true, true, true, true), 1, 2);
    assert_eq!(soap_for(&preflight, "customers"), SoapUse::Fallback);
    assert_eq!(soap_for(&preflight, "user_roles"), SoapUse::Preferred);
    assert_eq!(soap_for(&preflight, "users"), SoapUse::Required);
    assert_eq!(soap_for(&preflight, "access_groups"), SoapUse::Fallback);
    assert_eq!(soap_for(&preflight, "org_properties"), SoapUse::Fallback);
    assert_eq!(soap_for(&preflight, "device_properties"), SoapUse::None);
    assert!(preflight.uses_soap);
}

#[test]
fn a_rest_only_selection_does_not_need_soap() {
    let preflight = build_preflight(&options(false, false, false, false, false, true), 1, 2);
    assert!(!preflight.uses_soap);
    let preflight = build_preflight(&options(false, false, false, false, false, false), 1, 2);
    assert!(!preflight.uses_soap);
    assert_eq!(preflight.warnings.len(), 1, "an empty selection is flagged");
}

#[test]
fn access_group_membership_limits_are_stated_before_the_run() {
    let with_users = build_preflight(&options(true, false, true, true, false, false), 1, 2);
    let text = with_users.manual_follow_up.join("\n");
    assert!(
        text.contains("already exist on the destination are left unchanged"),
        "{}",
        text
    );
    assert!(!text.contains("Users is not selected"), "{}", text);

    let without_users = build_preflight(&options(true, false, false, true, false, false), 1, 2);
    let text = without_users.manual_follow_up.join("\n");
    assert!(text.contains("Users is not selected"), "{}", text);
    assert!(
        text.contains("without the source group's members"),
        "{}",
        text
    );

    let no_groups = build_preflight(&options(true, false, true, false, false, false), 1, 2);
    assert!(!no_groups
        .manual_follow_up
        .join("\n")
        .contains("access group"));
}

#[test]
fn skipping_customers_adds_a_read_only_matching_step_and_its_caveat() {
    let preflight = build_preflight(&options(false, true, false, false, false, false), 1, 2);
    assert_eq!(
        resources(&preflight),
        vec!["org_unit_mapping", "user_roles"]
    );
    assert_eq!(soap_for(&preflight, "org_unit_mapping"), SoapUse::None);
    assert!(preflight
        .manual_follow_up
        .join("\n")
        .contains("Customers is not selected"));

    let with_customers = build_preflight(&options(true, true, false, false, false, false), 1, 2);
    assert!(!resources(&with_customers).contains(&"org_unit_mapping"));
}

#[test]
fn property_steps_state_what_must_exist_on_the_destination_first() {
    let none = build_preflight(&options(true, true, true, true, false, false), 1, 2);
    assert!(none.prerequisites.is_empty());

    let org = build_preflight(&options(true, false, false, false, true, false), 1, 2);
    assert_eq!(org.prerequisites.len(), 1);
    assert!(org.prerequisites[0].contains("definitions must already exist"));
    assert!(org.prerequisites[0].contains("cannot create definitions"));

    let device = build_preflight(&options(false, false, false, false, false, true), 1, 2);
    let text = device.prerequisites.join("\n");
    assert!(
        text.contains("devices must already be on the destination"),
        "{}",
        text
    );
    assert!(text.contains("does not move or create devices"), "{}", text);
    assert!(text.contains("definitions must already exist"), "{}", text);
}
