use serde::{Deserialize, Serialize};

/// Device from /api/devices
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub device_id: i64,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub remote_control_uri: Option<String>,
    #[serde(default)]
    pub source_uri: Option<String>,
    #[serde(default)]
    pub long_name: Option<String>,
    #[serde(default)]
    pub device_class: Option<String>,
    #[serde(default)]
    pub device_class_label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_probe: Option<bool>,
    #[serde(default)]
    pub os_id: Option<String>,
    #[serde(default)]
    pub supported_os: Option<String>,
    #[serde(default)]
    pub supported_os_label: Option<String>,
    #[serde(default)]
    pub discovered_name: Option<String>,
    #[serde(default)]
    pub last_logged_in_user: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::models::common::option_bool_or_string"
    )]
    pub still_logged_in: Option<bool>,
    #[serde(default)]
    pub license_mode: Option<String>,
    #[serde(default)]
    pub org_unit_id: Option<i64>,
    #[serde(default)]
    pub so_id: Option<i64>,
    #[serde(default)]
    pub so_name: Option<String>,
    #[serde(default)]
    pub customer_id: Option<i64>,
    #[serde(default)]
    pub customer_name: Option<String>,
    #[serde(default)]
    pub site_id: Option<i64>,
    #[serde(default)]
    pub site_name: Option<String>,
    #[serde(default)]
    pub appliance_id: Option<i64>,
    #[serde(default)]
    pub last_appliance_checkin_time: Option<String>,
    #[serde(default)]
    pub last_appliance_checkin_time_local: Option<String>,
    #[serde(default)]
    pub device_status: Option<DeviceStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatus {
    #[serde(default)]
    pub state: Option<i32>,
    #[serde(default)]
    pub status: Option<i32>,
}

/// Device asset information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAsset {
    #[serde(default)]
    pub device_id: i64,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default, rename = "_links")]
    pub links: Option<std::collections::HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub computer_system: Option<ComputerSystem>,
    #[serde(default)]
    pub bios: Option<BiosInfo>,
    #[serde(default)]
    pub processor: Option<Vec<ProcessorInfo>>,
    #[serde(default)]
    pub memory: Option<MemoryInfo>,
    #[serde(default)]
    pub disk_drive: Option<Vec<DiskDriveInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerSystem {
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub domain_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiosInfo {
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub serial_number: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorInfo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub max_clock_speed: Option<i64>,
    #[serde(default)]
    pub number_of_cores: Option<i32>,
    #[serde(default)]
    pub number_of_logical_processors: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    #[serde(default)]
    pub total_physical_memory: Option<i64>,
    #[serde(default)]
    pub available_physical_memory: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskDriveInfo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub free_space: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::Device;

    #[test]
    fn parses_openapi_device_status_and_local_checkin_fields() {
        let device: Device = serde_json::from_value(serde_json::json!({
            "deviceId": 42,
            "lastApplianceCheckinTime": "2025-02-01T10:00:00Z",
            "lastApplianceCheckinTimeLocal": "2025-02-01T05:00:00-05:00",
            "deviceStatus": {
                "state": 1,
                "status": 2
            }
        }))
        .expect("OpenAPI device fields should deserialize");

        assert_eq!(
            device.last_appliance_checkin_time_local.as_deref(),
            Some("2025-02-01T05:00:00-05:00")
        );
        let status = device.device_status.expect("device status should be retained");
        assert_eq!(status.state, Some(1));
        assert_eq!(status.status, Some(2));
    }
}
