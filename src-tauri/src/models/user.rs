use crate::models::common::{option_string_or_i64, serialize_vec_to_string, string_or_i64};
use serde::{Deserialize, Serialize};

/// Extra fields from user API response (`_extra` object).
/// N-Central returns additional user details here (phone, department, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserExtra {
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub department: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Catch-all for any other fields
    #[serde(flatten)]
    pub other: std::collections::HashMap<String, serde_json::Value>,
}

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
    /// Extra fields containing phone, department, location, etc.
    #[serde(default, rename = "_extra")]
    pub extra: Option<UserExtra>,
}

/// Flattened User for CSV export. `UserExtra` uses `#[serde(flatten)]` over a
/// HashMap catch-all, which the csv crate can't serialize, so we pull the
/// known extra fields up to top-level columns and drop the catch-all.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCsvRow {
    pub user_id: i64,
    pub login_name: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub full_name: Option<String>,
    pub email: Option<String>,
    pub description: Option<String>,
    pub is_enabled: bool,
    pub is_ldap: bool,
    pub is_locked: Option<bool>,
    pub api_only_user: bool,
    pub logged_in_user: bool,
    pub read_only: bool,
    pub support_user: bool,
    pub two_factor_enabled: bool,
    pub current_sso_provider: Option<String>,
    pub access_group_ids: String,
    pub role_ids: String,
    pub customer_tree: String,
    pub created_on: Option<String>,
    pub org_unit_id: Option<i64>,
    pub service_org_id: Option<i64>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub location: Option<String>,
    pub title: Option<String>,
}

impl From<&User> for UserCsvRow {
    fn from(u: &User) -> Self {
        let join = |v: &[String]| v.join("; ");
        let join_ids = |v: &[i64]| {
            v.iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        };
        let (phone, department, location, title) = match &u.extra {
            Some(e) => (
                e.phone.clone(),
                e.department.clone(),
                e.location.clone(),
                e.title.clone(),
            ),
            None => (None, None, None, None),
        };
        Self {
            user_id: u.user_id,
            login_name: u.login_name.clone(),
            first_name: u.first_name.clone(),
            last_name: u.last_name.clone(),
            full_name: u.full_name.clone(),
            email: u.email.clone(),
            description: u.description.clone(),
            is_enabled: u.is_enabled,
            is_ldap: u.is_ldap,
            is_locked: u.is_locked,
            api_only_user: u.api_only_user,
            logged_in_user: u.logged_in_user,
            read_only: u.read_only,
            support_user: u.support_user,
            two_factor_enabled: u.two_factor_enabled,
            current_sso_provider: u.current_sso_provider.clone(),
            access_group_ids: join_ids(&u.access_group_ids),
            role_ids: join_ids(&u.role_ids),
            customer_tree: join(&u.customer_tree),
            created_on: u.created_on.clone(),
            org_unit_id: u.org_unit_id,
            service_org_id: u.service_org_id,
            phone,
            department,
            location,
            title,
        }
    }
}
