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
    #[serde(default)]
    pub generation: Option<i32>,
    #[serde(default)]
    pub descendant_count: Option<i32>,
    /// Extra fields from API response. Skipped on serialize because the csv
    /// crate can't emit maps and nothing in this project reads these keys.
    #[serde(default, rename = "_extra", skip_serializing)]
    pub extra: Option<std::collections::HashMap<String, serde_json::Value>>,
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
    #[serde(default)]
    pub generation: Option<i32>,
    #[serde(default)]
    pub descendant_count: Option<i32>,
    
    // Potentially alternate names for parent linkage
    #[serde(default, alias = "customer_id", deserialize_with = "option_string_or_i64")]
    pub customer_id: Option<i64>,
    #[serde(default, deserialize_with = "option_string_or_i64")]
    pub customerid: Option<i64>,
    #[serde(default, alias = "org_unit_id", deserialize_with = "option_string_or_i64")]
    pub org_unit_id: Option<i64>,
    #[serde(default, alias = "service_org_id", deserialize_with = "option_string_or_i64")]
    pub service_org_id: Option<i64>,
    #[serde(default, alias = "service_orgid", deserialize_with = "option_string_or_i64")]
    pub service_orgid: Option<i64>,
    /// Extra fields from API response. Skipped on serialize because the csv
    /// crate can't emit maps and nothing in this project reads these keys.
    #[serde(default, rename = "_extra", skip_serializing)]
    pub extra: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[cfg(test)]
mod tests {
    use super::{Customer, Site};

    #[test]
    fn parses_openapi_customer_and_site_examples() {
        let customer: Customer = serde_json::from_value(serde_json::json!({
            "customerId": "100",
            "customerName": "N-able",
            "orgUnitType": "SO",
            "parentId": "50",
            "externalId": "extId",
            "externalId2": "extId2",
            "contactPhoneExt": null,
            "contactDepartment": "Support",
            "county": null,
            "postalCode": "A1A 1A1"
        }))
        .expect("OpenAPI customer example should deserialize");
        let site: Site = serde_json::from_value(serde_json::json!({
            "siteId": "100",
            "siteName": "N-able",
            "orgUnitType": "SO",
            "parentId": "50",
            "externalId": "extId",
            "externalId2": "extId2",
            "contactPhoneExt": null,
            "contactDepartment": "Support",
            "county": null,
            "postalCode": "A1A 1A1"
        }))
        .expect("OpenAPI site example should deserialize");

        assert_eq!(customer.customer_id, 100);
        assert_eq!(customer.parent_id, Some(50));
        assert_eq!(customer.external_id2.as_deref(), Some("extId2"));
        assert_eq!(site.site_id, 100);
        assert_eq!(site.parent_id, Some(50));
        assert_eq!(site.external_id2.as_deref(), Some("extId2"));
    }
}
