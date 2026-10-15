//! Custom Property models

use serde::{Deserialize, Serialize};

/// Organization custom property from /api/org-units/{id}/custom-properties
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgProperty {
    #[serde(deserialize_with = "crate::models::common::string_or_i64")]
    pub property_id: i64,
    #[serde(default, rename = "propertyName", alias = "label")]
    pub label: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub property_type: Option<String>,
    #[serde(default)]
    pub org_unit_id: Option<i64>,
    #[serde(default)]
    pub enumerated_value_list: Option<Vec<String>>,
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
    #[serde(default, rename = "propertyName", alias = "label")]
    pub label: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub property_type: Option<String>,
    #[serde(default)]
    pub enumerated_value_list: Option<Vec<String>>,
    /// Extra fields from API response. Skipped on serialize because the csv
    /// crate can't emit maps and nothing in this project reads these keys.
    #[serde(default, rename = "_extra", skip_serializing)]
    pub extra: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgPropertyCsvRow {
    pub property_id: i64,
    pub label: Option<String>,
    pub value: Option<String>,
    pub default_value: Option<String>,
    pub property_type: Option<String>,
    pub org_unit_id: Option<i64>,
    pub enumerated_value_list: String,
}

impl From<&OrgProperty> for OrgPropertyCsvRow {
    fn from(property: &OrgProperty) -> Self {
        Self {
            property_id: property.property_id,
            label: property.label.clone(),
            value: property.value.clone(),
            default_value: property.default_value.clone(),
            property_type: property.property_type.clone(),
            org_unit_id: property.org_unit_id,
            enumerated_value_list: property
                .enumerated_value_list
                .as_deref()
                .unwrap_or_default()
                .join(";"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePropertyCsvRow {
    pub property_id: i64,
    pub device_id: Option<i64>,
    pub device_name: Option<String>,
    pub label: Option<String>,
    pub value: Option<String>,
    pub default_value: Option<String>,
    pub property_type: Option<String>,
    pub enumerated_value_list: String,
}

impl From<&DeviceProperty> for DevicePropertyCsvRow {
    fn from(property: &DeviceProperty) -> Self {
        Self {
            property_id: property.property_id,
            device_id: property.device_id,
            device_name: property.device_name.clone(),
            label: property.label.clone(),
            value: property.value.clone(),
            default_value: property.default_value.clone(),
            property_type: property.property_type.clone(),
            enumerated_value_list: property
                .enumerated_value_list
                .as_deref()
                .unwrap_or_default()
                .join(";"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceProperty, DevicePropertyCsvRow, OrgProperty, OrgPropertyCsvRow};

    #[test]
    fn accepts_openapi_property_name_for_org_and_device_properties() {
        let org_property: OrgProperty = serde_json::from_value(serde_json::json!({
            "propertyId": 1,
            "propertyName": "Department",
            "value": "Operations",
            "enumeratedValueList": ["Operations", "Sales"]
        }))
        .expect("organization property should deserialize");
        let device_property: DeviceProperty = serde_json::from_value(serde_json::json!({
            "propertyId": 2,
            "propertyName": "Asset Tag",
            "value": "ABC-123",
            "enumeratedValueList": ["ABC-123", "DEF-456"]
        }))
        .expect("device property should deserialize");

        assert_eq!(org_property.label.as_deref(), Some("Department"));
        assert_eq!(device_property.label.as_deref(), Some("Asset Tag"));
        assert_eq!(
            serde_json::to_value(&org_property).expect("property serializes")["propertyName"],
            "Department"
        );
        assert_eq!(
            OrgPropertyCsvRow::from(&org_property).enumerated_value_list,
            "Operations;Sales"
        );
        assert_eq!(
            DevicePropertyCsvRow::from(&device_property).enumerated_value_list,
            "ABC-123;DEF-456"
        );
        assert_eq!(
            serde_json::to_value(&device_property).expect("property serializes")
                ["enumeratedValueList"],
            serde_json::json!(["ABC-123", "DEF-456"])
        );
        assert_eq!(
            serde_json::to_value(&device_property).expect("property serializes")["propertyName"],
            "Asset Tag"
        );
    }
}
