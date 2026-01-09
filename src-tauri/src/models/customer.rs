//! Customer and Site models

use serde::{Deserialize, Serialize};
use crate::models::common::{string_or_i64, option_string_or_i64};

/// Customer from /api/customers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Customer {
    #[serde(deserialize_with = "string_or_i64")]
    pub customer_id: i64,
    pub customer_name: String,
    #[serde(default)]
    pub org_unit_type: Option<String>,
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub external_id2: Option<String>,
    #[serde(default)]
    pub contact_first_name: Option<String>,
    #[serde(default)]
    pub contact_last_name: Option<String>,
    #[serde(default)]
    pub contact_email: Option<String>,
    #[serde(default)]
    pub contact_phone: Option<String>,
    #[serde(default)]
    pub contact_phone_ext: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub contact_title: Option<String>,
    #[serde(default)]
    pub contact_department: Option<String>,
    #[serde(default)]
    pub county: Option<String>,
    #[serde(default)]
    pub street1: Option<String>,
    #[serde(default)]
    pub street2: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub state_prov: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
    #[serde(default)]
    pub is_system: Option<bool>,
    #[serde(default)]
    pub is_service_org: Option<bool>,
}

/// Site from /api/sites
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    #[serde(deserialize_with = "string_or_i64")]
    pub site_id: i64,
    pub site_name: String,
    #[serde(default)]
    pub org_unit_type: Option<String>,
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub external_id2: Option<String>,
    #[serde(default)]
    pub contact_first_name: Option<String>,
    #[serde(default)]
    pub contact_last_name: Option<String>,
    #[serde(default)]
    pub contact_email: Option<String>,
    #[serde(default)]
    pub contact_phone: Option<String>,
    #[serde(default)]
    pub contact_phone_ext: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub contact_title: Option<String>,
    #[serde(default)]
    pub contact_department: Option<String>,
    #[serde(default)]
    pub county: Option<String>,
    #[serde(default)]
    pub street1: Option<String>,
    #[serde(default)]
    pub street2: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub state_prov: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
    #[serde(default)]
    pub is_system: Option<bool>,
    #[serde(default)]
    pub is_service_org: Option<bool>,
    
    // Potentially alternate names for parent linkage
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub customer_id: Option<i64>,
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub customerid: Option<i64>,
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub org_unit_id: Option<i64>,
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub service_org_id: Option<i64>,
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub service_orgid: Option<i64>,
}
