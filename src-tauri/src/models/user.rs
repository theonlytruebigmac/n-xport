use crate::models::common::{option_string_or_i64, serialize_vec_to_string, string_or_i64};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    #[serde(deserialize_with = "string_or_i64")]
    pub user_id: i64,
    #[serde(rename = "userName")]
    pub login_name: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub full_name: Option<String>,
    pub email: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    pub is_ldap: bool,
    pub is_locked: Option<bool>,
    pub api_only_user: bool,
    pub logged_in_user: bool,
    pub read_only: bool,
    pub support_user: bool,
    pub two_factor_enabled: bool,
    pub current_sso_provider: Option<String>,
    #[serde(serialize_with = "serialize_vec_to_string")]
    pub access_group_ids: Vec<i64>,
    #[serde(serialize_with = "serialize_vec_to_string")]
    pub role_ids: Vec<i64>,
    #[serde(serialize_with = "serialize_vec_to_string")]
    pub customer_tree: Vec<String>,
    pub created_on: Option<String>,
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub org_unit_id: Option<i64>,
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub service_org_id: Option<i64>,
}
