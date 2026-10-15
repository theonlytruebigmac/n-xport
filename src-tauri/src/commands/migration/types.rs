//! The migration request and result types, and the source-to-destination ID mapping.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub success: bool,
    pub cancelled: bool,
    pub issues: Vec<String>,
    pub skipped: Vec<String>,
    pub manual_follow_up: Vec<String>,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MigrationOptions {
    pub customers: bool,
    pub user_roles: bool,
    pub access_groups: bool,
    pub users: bool,
    pub org_properties: bool,
    #[serde(default)]
    pub device_properties: bool,
}

impl MigrationOptions {
    /// Roles, users, access groups, and properties all walk the customer/site
    /// mapping. When customers are not being migrated, that mapping has to be
    /// built from the customers and sites that already exist on the destination;
    /// otherwise those resources would silently cover only the service org.
    pub(crate) fn needs_existing_org_unit_mapping(&self) -> bool {
        !self.customers
            && (self.user_roles
                || self.users
                || self.access_groups
                || self.org_properties
                || self.device_properties)
    }
}

/// Mapping of Source IDs to Destination IDs and name-based lookups
pub struct IdMapping {
    /// Source Customer ID -> Destination Customer ID
    pub customers: HashMap<i64, i64>,
    /// Source Site ID -> Destination Site ID
    pub sites: HashMap<i64, i64>,
    /// Source Role ID -> Destination Role ID
    pub roles: HashMap<i64, i64>,
    /// Source Access Group ID -> Destination Access Group ID
    pub access_groups: HashMap<i64, i64>,
    /// Role Name (lowercase) -> Destination Role ID (fallback for user creation)
    pub role_names: HashMap<String, i64>,
    /// User Login Name (lowercase) -> Destination User ID
    pub user_logins: HashMap<String, i64>,
    /// Source Org Unit ID -> Destination Org Unit ID (SO, Customer, or Site)
    pub org_units: HashMap<i64, i64>,
    /// Source Access Group ID -> list of Destination User IDs that should be members.
    /// Built during user migration from each source user's access_group_ids field.
    pub access_group_members: HashMap<i64, Vec<i64>>,
}

impl Default for IdMapping {
    fn default() -> Self {
        Self::new()
    }
}

impl IdMapping {
    pub fn new() -> Self {
        Self {
            customers: HashMap::new(),
            sites: HashMap::new(),
            roles: HashMap::new(),
            access_groups: HashMap::new(),
            role_names: HashMap::new(),
            user_logins: HashMap::new(),
            org_units: HashMap::new(),
            access_group_members: HashMap::new(),
        }
    }
}
