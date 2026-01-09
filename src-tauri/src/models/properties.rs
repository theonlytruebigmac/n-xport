//! Custom Property models

use serde::{Deserialize, Serialize};

/// Organization custom property from /api/org-units/{id}/custom-properties
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgProperty {
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
}

/// Device custom property from /api/devices/{id}/custom-properties
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProperty {
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
}
