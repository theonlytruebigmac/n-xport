//! User Role models

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Extra fields from user role API response
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRoleExtra {
    /// Whether the role is read-only (system role)
    #[serde(default)]
    pub readonly: Option<String>,
    /// Permission names assigned to this role
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Usernames assigned to this role
    #[serde(default)]
    pub usernames: Vec<String>,
    /// Whether the role can be cloned
    #[serde(default)]
    pub cloneable: Option<String>,
}

/// User Role from /api/org-units/{id}/user-roles
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRole {
    /// The actual field name from N-Central is "roleId"
    #[serde(alias = "userRoleId", deserialize_with = "crate::models::common::string_or_i64")]
    pub role_id: i64,
    #[serde(default)]
    pub role_name: Option<String>,
    #[serde(default)]
    pub role_description: Option<String>,
    /// Extra fields containing permissions, usernames, etc.
    #[serde(default, rename = "_extra")]
    pub extra: Option<UserRoleExtra>,
}

impl UserRole {
    /// Get the permission names for this role
    pub fn get_permissions(&self) -> Vec<String> {
        self.extra
            .as_ref()
            .map(|e| e.permissions.clone())
            .unwrap_or_default()
    }

    /// Get the usernames assigned to this role from the _extra field
    pub fn get_usernames(&self) -> Vec<String> {
        self.extra
            .as_ref()
            .map(|e| e.usernames.clone())
            .unwrap_or_default()
    }
}

/// Flattened UserRole for CSV export. The `permissions` and `usernames`
/// vectors vary in length per role, so CSV writes fail with mismatched
/// field counts unless we join them into delimited strings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRoleCsvRow {
    pub role_id: i64,
    pub role_name: Option<String>,
    pub role_description: Option<String>,
    pub readonly: Option<String>,
    pub cloneable: Option<String>,
    pub permissions: String,
    pub usernames: String,
}

impl From<&UserRole> for UserRoleCsvRow {
    fn from(ur: &UserRole) -> Self {
        let permissions = ur.get_permissions().join(";");
        let usernames = ur.get_usernames().join(";");
        let (readonly, cloneable) = match &ur.extra {
            Some(e) => (e.readonly.clone(), e.cloneable.clone()),
            None => (None, None),
        };
        Self {
            role_id: ur.role_id,
            role_name: ur.role_name.clone(),
            role_description: ur.role_description.clone(),
            readonly,
            cloneable,
            permissions,
            usernames,
        }
    }
}

/// Mapping of permission name to permission ID
/// Loaded from rolePermissionIds.csv
pub struct PermissionLookup {
    name_to_id: HashMap<String, i64>,
}

impl PermissionLookup {
    /// Create a new empty lookup
    pub fn new() -> Self {
        Self {
            name_to_id: HashMap::new(),
        }
    }

    /// Load permission mappings from CSV content
    pub fn from_csv(csv_content: &str) -> Self {
        let mut lookup = Self::new();

        // Skip header line
        for line in csv_content.lines().skip(1) {
            // Parse CSV: "groupid","permissionid","permissionname",...
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 3 {
                // Remove quotes from fields
                let perm_id_str = parts[1].trim().trim_matches('"');
                let perm_name = parts[2].trim().trim_matches('"');

                if let Ok(perm_id) = perm_id_str.parse::<i64>() {
                    lookup.name_to_id.insert(perm_name.to_string(), perm_id);
                }
            }
        }

        tracing::info!("Loaded {} permission mappings", lookup.name_to_id.len());
        lookup
    }

    /// Get permission ID by name
    pub fn get_id(&self, name: &str) -> Option<i64> {
        self.name_to_id.get(name).copied()
    }

    /// Convert a list of permission names to permission IDs
    pub fn names_to_ids(&self, names: &[String]) -> Vec<i64> {
        names
            .iter()
            .filter_map(|name| {
                let id = self.get_id(name);
                if id.is_none() {
                    tracing::warn!("Unknown permission name: {}", name);
                }
                id
            })
            .collect()
    }

    /// Check if the lookup has any mappings
    pub fn is_empty(&self) -> bool {
        self.name_to_id.is_empty()
    }
}
