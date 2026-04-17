//! Custom Property models

use serde::{Deserialize, Serialize};

/// Organization custom property from /api/org-units/{id}/custom-properties
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgProperty {
    #[serde(deserialize_with = "crate::models::common::string_or_i64")]
    pub property_id: i64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub property_type: Option<String>,
    #[serde(default)]
    pub org_unit_id: Option<i64>,
    /// Extra fields from API response. Skipped on serialize because the csv
    /// crate can't emit maps and nothing in this project reads these keys.
    #[serde(default, rename = "_extra", skip_serializing)]
    pub extra: Option<std::collections::HashMap<String, serde_json::Value>>,
}

/// Device custom property from /api/devices/{id}/custom-properties
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProperty {
    #[serde(deserialize_with = "crate::models::common::string_or_i64")]
    pub property_id: i64,
    #[serde(default)]
    pub device_id: Option<i64>,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub property_type: Option<String>,
    /// Extra fields from API response. Skipped on serialize because the csv
    /// crate can't emit maps and nothing in this project reads these keys.
    #[serde(default, rename = "_extra", skip_serializing)]
    pub extra: Option<std::collections::HashMap<String, serde_json::Value>>,
}
