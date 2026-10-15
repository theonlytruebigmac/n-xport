use super::{PermissionLookup, UserRole, UserRoleCsvRow};

#[test]
fn the_bundled_permission_list_covers_current_and_alias_permissions_with_unique_names() {
    let csv = include_str!("../../../data/rolePermissionIds.csv");
    let lookup = PermissionLookup::bundled();

    // Newer permissions that the list once lacked, and alias names that share an ID.
    for name in [
        "AI_ALL",
        "COWORKERS_MANAGE",
        "AUDIT_STREAMING_ALL",
        "RSS_VIEW",
    ] {
        assert!(lookup.get_id(name).is_some(), "{} should resolve", name);
    }
    assert_eq!(
        lookup.get_id("NCENTRAL_INTEGRATION_MANAGEMENT_ALL"),
        lookup.get_id("NCENTRAL_INTEGRATION_MANAGEMENT"),
        "an alias resolves to the same ID as the permission it aliases"
    );

    // Every data row parses and each name appears once (a repeat would silently override).
    let names: Vec<&str> = csv
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.split(',')
                .nth(2)
                .expect("name column")
                .trim_matches('"')
        })
        .collect();
    let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
    assert_eq!(
        names.len(),
        unique.len(),
        "duplicate permission names in the CSV"
    );
    assert_eq!(unique.len(), 191);
}

#[test]
fn parses_openapi_role_details_and_boolean_extra_fields() {
    let role: UserRole = serde_json::from_value(serde_json::json!({
        "roleId": 1001,
        "orgUnitId": 1,
        "roleName": "Default Dashboard Role",
        "roleDescription": "Read-only dashboard access",
        "orgUnitId": 1,
        "userIds": [12, 13],
        "_extra": {
            "cloneable": true,
            "readonly": true,
            "permissions": ["DASHBOARDS_VIEW", "SERVICE_VIEW"],
            "source": "system"
        }
    }))
    .expect("OpenAPI role details should deserialize");

    let extra = role.extra.as_ref().expect("role extra should be retained");
    assert_eq!(extra.cloneable, Some(true));
    assert_eq!(extra.readonly, Some(true));
    assert_eq!(role.org_unit_id, Some(1));
    assert_eq!(extra.other["source"], "system");
    assert_eq!(role.user_ids, Some(vec![12, 13]));
    assert_eq!(
        role.get_permissions(),
        vec!["DASHBOARDS_VIEW", "SERVICE_VIEW"]
    );
    let json = serde_json::to_value(&role).expect("role should serialize");
    assert_eq!(json["_extra"]["source"], "system");
    let row = UserRoleCsvRow::from(&role);
    assert_eq!(row.readonly, Some(true));
    assert_eq!(row.user_ids, "12;13");
    assert_eq!(row.org_unit_id, Some(1));
}
