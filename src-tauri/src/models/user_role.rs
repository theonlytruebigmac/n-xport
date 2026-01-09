//! User Role models

use serde::{Deserialize, Serialize};

/// User Role from /api/org-units/{id}/user-roles
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRole {
    /// The actual field name from N-Central is "roleId"
    #[serde(alias = "userRoleId")]
    pub role_id: i64,
    #[serde(default)]
    pub role_name: Option<String>,
    #[serde(default)]
    pub role_description: Option<String>,
}
