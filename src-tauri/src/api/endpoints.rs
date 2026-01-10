//! REST API endpoints for N-Central

use serde::Serialize;

/// API endpoint paths
pub mod paths {
    // Authentication
    pub const AUTH_AUTHENTICATE: &str = "/api/auth/authenticate";
    pub const AUTH_REFRESH: &str = "/api/auth/refresh";
    pub const AUTH_VALIDATE: &str = "/api/auth/validate";

    // Server info
    pub const SERVER_INFO: &str = "/api/server-info";
    pub const HEALTH: &str = "/api/health";

    // Service Organizations
    pub const SERVICE_ORGS: &str = "/api/service-orgs";

    // Customers
    pub const CUSTOMERS: &str = "/api/customers";

    // Sites
    pub const SITES: &str = "/api/sites";

    // Devices
    pub const DEVICES: &str = "/api/devices";

    // Organization Units
    pub const ORG_UNITS: &str = "/api/org-units";

    // Users
    pub const USERS: &str = "/api/users";

    // Custom Properties
    pub const CUSTOM_PROPERTIES_VALUES: &str = "/api/custom-properties/values";
}

/// Build URL for a single service org by ID
pub fn service_org_by_id(so_id: i64) -> String {
    format!("/api/service-orgs/{}", so_id)
}

/// Build URL for service org customers
pub fn service_org_customers(so_id: i64) -> String {
    format!("/api/service-orgs/{}/customers", so_id)
}

/// Build URL for service org sites (via org-units)
pub fn service_org_sites(_so_id: i64) -> String {
    format!("/api/sites")
}

/// Build URL for org unit access groups (GET list)
pub fn org_unit_access_groups(org_unit_id: i64) -> String {
    format!("/api/org-units/{}/access-groups", org_unit_id)
}

/// Build URL for creating org unit type access groups
pub fn org_unit_access_groups_create(org_unit_id: i64) -> String {
    format!("/api/org-units/{}/org-unit-access-groups", org_unit_id)
}

/// Build URL for creating device type access groups
pub fn device_access_groups_create(org_unit_id: i64) -> String {
    format!("/api/org-units/{}/device-access-groups", org_unit_id)
}

/// Build URL for org unit user roles
pub fn org_unit_user_roles(org_unit_id: i64) -> String {
    format!("/api/org-units/{}/user-roles", org_unit_id)
}

/// Build URL for org unit custom properties
pub fn org_unit_custom_properties(org_unit_id: i64) -> String {
    format!("/api/org-units/{}/custom-properties", org_unit_id)
}

/// Build URL for org unit users
pub fn org_unit_users(org_unit_id: i64) -> String {
    format!("/api/org-units/{}/users", org_unit_id)
}

/// Build URL for org unit devices
pub fn org_unit_devices(org_unit_id: i64) -> String {
    format!("/api/org-units/{}/devices", org_unit_id)
}

/// Build URL for device by ID
pub fn device_by_id(device_id: i64) -> String {
    format!("/api/devices/{}", device_id)
}

/// Build URL for device custom properties
pub fn device_custom_properties(device_id: i64) -> String {
    format!("/api/devices/{}/custom-properties", device_id)
}

/// Build URL for device assets
pub fn device_assets(device_id: i64) -> String {
    format!("/api/devices/{}/assets", device_id)
}

/// Pagination query parameters
#[derive(Debug, Clone, Serialize, Default)]
pub struct PaginationParams {
    #[serde(rename = "pageNumber", skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(rename = "pageSize", skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    #[serde(rename = "sortBy", skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<String>,
    #[serde(rename = "sortOrder", skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<String>,
}

impl PaginationParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    pub fn page_size(mut self, size: u32) -> Self {
        self.page_size = Some(size);
        self
    }
}
