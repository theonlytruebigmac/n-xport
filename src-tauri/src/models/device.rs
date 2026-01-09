use serde::{Deserialize, Serialize, Deserializer};

fn option_string_or_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    
    struct OptionStringOrBool;
    
    impl<'de> Visitor<'de> for OptionStringOrBool {
        type Value = Option<bool>;
        
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("null, string or boolean")
        }
        
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(Some(v))
        }
        
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            match v.to_lowercase().as_str() {
                "true" | "1" | "yes" => Ok(Some(true)),
                "false" | "0" | "no" => Ok(Some(false)),
                "" => Ok(None),
                _ => Err(de::Error::custom(format!("invalid boolean: {}", v)))
            }
        }
    }
    
    deserializer.deserialize_any(OptionStringOrBool)
}

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
    #[serde(default, deserialize_with = "option_string_or_bool")]
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
}

/// Device asset information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAsset {
    pub device_id: i64,
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
