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
    #[serde(default, rename = "autoIncludeNewOrgUnits")]
    pub auto_include_new_org_units: Option<String>,
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
        self.extra
            .as_ref()
            .map(|e| e.org_unit_ids.clone())
            .unwrap_or_default()
    }

    /// Get the auto-include setting from the _extra field
    pub fn get_auto_include(&self) -> Option<String> {
        self.extra
            .as_ref()
            .and_then(|e| e.auto_include_new_org_units.clone())
    }
}
