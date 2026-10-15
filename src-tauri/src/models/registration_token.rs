//! Agent registration tokens for customers and sites

use serde::{Deserialize, Serialize};

/// A registration token as returned by `GET /api/org-units/{id}/registration-token`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationToken {
    #[serde(default)]
    pub registration_token: Option<String>,
    #[serde(default)]
    pub registration_token_expiry_date: Option<String>,
}

impl RegistrationToken {
    /// The API documents the token inside a `data` envelope; accept it bare as well.
    pub fn from_response(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        let inner = match value {
            serde_json::Value::Object(mut map) if map.contains_key("data") => {
                map.remove("data").unwrap_or_default()
            }
            other => other,
        };
        serde_json::from_value(inner)
    }
}

/// One exported row: the token of a customer or site, with where it belongs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationTokenRow {
    pub org_unit_id: i64,
    /// `customer` or `site`.
    pub org_unit_type: String,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub site_id: Option<i64>,
    pub site_name: Option<String>,
    pub registration_token: Option<String>,
    pub registration_token_expiry_date: Option<String>,
}
