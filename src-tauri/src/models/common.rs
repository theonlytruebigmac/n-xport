use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Helper to serialize a Vec as a semicolon-separated string for CSV
pub fn serialize_vec_to_string<S, T>(v: &Vec<T>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: std::fmt::Display,
{
    let combined = v
        .iter()
        .map(|item| item.to_string())
        .collect::<Vec<String>>()
        .join("; ");
    s.serialize_str(&combined)
}

/// Helper to serialize an Option<Vec> as a semicolon-separated string for CSV
pub fn serialize_opt_vec_to_string<S, T>(v: &Option<Vec<T>>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: std::fmt::Display,
{
    match v {
        Some(vec) if !vec.is_empty() => {
            let combined = vec
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<String>>()
                .join("; ");
            s.serialize_str(&combined)
        }
        _ => s.serialize_none(),
    }
}

/// Helper to deserialize string or number as i64
pub fn string_or_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct StringOrInt;

    impl<'de> Visitor<'de> for StringOrInt {
        type Value = i64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("string or integer")
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(v)
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(v as i64)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            v.parse().map_err(de::Error::custom)
        }
    }

    deserializer.deserialize_any(StringOrInt)
}

/// Helper to deserialize optional string or number as i64
pub fn option_string_or_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct OptionStringOrInt;

    impl<'de> Visitor<'de> for OptionStringOrInt {
        type Value = Option<i64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("null, string or integer")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v as i64))
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            if v.is_empty() {
                Ok(None)
            } else {
                v.parse().map(Some).map_err(de::Error::custom)
            }
        }
    }

    deserializer.deserialize_any(OptionStringOrInt)
}

/// Pagination information from API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
    pub total_items: u32,
}

/// Standard paginated response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    #[serde(flatten)]
    pub page_info: Option<PageInfo>,
}

/// HATEOAS link from API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiLink {
    pub rel: String,
    pub href: String,
    #[serde(rename = "type")]
    pub link_type: Option<String>,
}

/// Export options selected by user
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub service_orgs: bool,
    pub customers: bool,
    pub sites: bool,
    pub devices: bool,
    pub access_groups: bool,
    pub user_roles: bool,
    pub org_properties: bool,
    pub device_properties: bool,
    pub users: bool,
    pub device_assets: bool,
}

/// Export format
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Json,
}

impl Default for ExportFormat {
    fn default() -> Self {
        Self::Csv
    }
}

/// Progress update for UI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressUpdate {
    pub phase: String,
    pub message: String,
    pub percent: f32,
    pub current: u32,
    pub total: u32,
}

/// Log message for UI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogMessage {
    pub level: String,
    pub message: String,
}
