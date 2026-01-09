//! Access Group models

use serde::{Deserialize, Serialize};

/// Access Group from /api/org-units/{id}/access-groups
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessGroup {
    /// The actual field name from N-Central is "groupId"
    #[serde(alias = "accessGroupId")]
    pub group_id: i64,
    #[serde(default)]
    pub org_unit_id: Option<i64>,
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default)]
    pub group_type: Option<String>,
    #[serde(default)]
    pub group_description: Option<String>,
}
