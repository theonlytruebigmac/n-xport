//! Access Group models

use serde::{Deserialize, Serialize};

/// Extra fields from access group API response (`_extra` object).
/// N-Central returns member usernames, org unit scope, and other metadata here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessGroupExtra {
    /// Usernames that are members of this access group
    #[serde(default)]
    pub usernames: Vec<String>,
    /// Org unit IDs this access group covers
    #[serde(default, rename = "orgUnitIds")]
    pub org_unit_ids: Vec<i64>,
    /// Whether new org units are automatically included
    #[serde(
        default,
        rename = "autoIncludeNewOrgUnits",
        deserialize_with = "crate::models::common::option_bool_or_string"
    )]
    pub auto_include_new_org_units: Option<bool>,
    /// Catch-all for any other fields
    #[serde(flatten)]
    pub other: std::collections::HashMap<String, serde_json::Value>,
}

/// Access Group from /api/org-units/{id}/access-groups
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessGroup {
    /// The actual field name from N-Central is "groupId"
    #[serde(alias = "accessGroupId", deserialize_with = "crate::models::common::string_or_i64")]
    pub group_id: i64,
    #[serde(default)]
    pub org_unit_id: Option<i64>,
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default)]
    pub group_type: Option<String>,
    #[serde(default)]
    pub group_description: Option<String>,
    #[serde(default)]
    pub org_unit_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub device_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub user_ids: Option<Vec<i64>>,
    #[serde(
        default,
        deserialize_with = "crate::models::common::option_bool_or_string"
    )]
    pub auto_include_new_org_units: Option<bool>,
    /// Extra fields containing member usernames, org unit scope, etc.
    #[serde(default, rename = "_extra")]
    pub extra: Option<AccessGroupExtra>,
}

impl AccessGroup {
    /// Get the member usernames from the _extra field
    pub fn get_usernames(&self) -> Vec<String> {
        self.extra
            .as_ref()
            .map(|e| e.usernames.clone())
            .unwrap_or_default()
    }

    /// Get the org unit IDs this group covers from the _extra field
    pub fn get_org_unit_ids(&self) -> Vec<i64> {
        self.org_unit_ids.clone().unwrap_or_else(|| {
            self.extra
                .as_ref()
                .map(|e| e.org_unit_ids.clone())
                .unwrap_or_default()
        })
    }

    /// Get the auto-include setting, preferring the documented top-level field.
    pub fn get_auto_include(&self) -> Option<bool> {
        self.auto_include_new_org_units.or_else(|| {
            self.extra
                .as_ref()
                .and_then(|e| e.auto_include_new_org_units)
        })
    }

    pub fn get_group_type(&self) -> Option<String> {
        self.group_type.clone().or_else(|| {
            self.extra.as_ref().and_then(|extra| {
                extra
                    .other
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case("grouptype"))
                    .and_then(|(_, value)| value.as_str())
                    .map(str::to_string)
            })
        })
    }
}

/// Flattened AccessGroup for CSV export. CSV can't serialize nested vecs/maps,
/// so list fields are joined with `;` and the catch-all `other` map is dropped.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessGroupCsvRow {
    pub group_id: i64,
    pub org_unit_id: Option<i64>,
    pub group_name: Option<String>,
    pub group_type: Option<String>,
    pub group_description: Option<String>,
    pub usernames: String,
    pub org_unit_ids: String,
    pub device_ids: String,
    pub user_ids: String,
    pub auto_include_new_org_units: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::{AccessGroup, AccessGroupCsvRow};

    #[test]
    fn parses_openapi_access_group_details_without_dropping_membership_fields() {
        let group: AccessGroup = serde_json::from_value(serde_json::json!({
            "groupId": 1549311915,
            "orgUnitId": 1,
            "groupName": "Technicians",
            "groupDescription": "Support staff",
            "orgUnitIds": [1, 50, 102],
            "deviceIds": [200, 201],
            "userIds": [300],
            "autoIncludeNewOrgUnits": false,
            "_extra": {
                "grouptype": "ORG_UNIT",
                "readonly": "false"
            }
        }))
        .expect("OpenAPI access group details should deserialize");

        let row = AccessGroupCsvRow::from(&group);
        assert_eq!(row.org_unit_ids, "1;50;102");
        assert_eq!(row.device_ids, "200;201");
        assert_eq!(row.user_ids, "300");
        assert_eq!(row.group_type.as_deref(), Some("ORG_UNIT"));
        assert_eq!(row.auto_include_new_org_units, Some(false));
        assert_eq!(group.device_ids, Some(vec![200, 201]));
        assert_eq!(group.user_ids, Some(vec![300]));
    }
}

impl From<&AccessGroup> for AccessGroupCsvRow {
    fn from(ag: &AccessGroup) -> Self {
        let usernames = ag.get_usernames().join(";");
        let org_unit_ids = ag
            .get_org_unit_ids()
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(";");
        let device_ids = ag
            .device_ids
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(";");
        let user_ids = ag
            .user_ids
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(";");
        Self {
            group_id: ag.group_id,
            org_unit_id: ag.org_unit_id,
            group_name: ag.group_name.clone(),
            group_type: ag.get_group_type(),
            group_description: ag.group_description.clone(),
            usernames,
            org_unit_ids,
            device_ids,
            user_ids,
            auto_include_new_org_units: ag.get_auto_include(),
        }
    }
}
