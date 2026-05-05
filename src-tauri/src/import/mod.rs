//! CSV import functionality.
//!
//! Defines the shape of each importable resource's CSV (one struct per resource),
//! a template generator that writes a header row plus an example row for users to
//! fill in, and a parser that returns Vec<Row> for downstream handlers.

pub mod handlers;

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::{AppError, Result};

/// CSV shape for the Customers import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerImportRow {
    /// Required. The customer name to create.
    pub customer_name: String,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub contact_first_name: Option<String>,
    #[serde(default)]
    pub contact_last_name: Option<String>,
    #[serde(default)]
    pub contact_email: Option<String>,
    #[serde(default)]
    pub contact_phone: Option<String>,
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
}

/// CSV shape for the Sites import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteImportRow {
    /// Required. The parent customer's name (looked up under the target SO).
    pub customer_name: String,
    /// Required. The site name to create under the parent customer.
    pub site_name: String,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub contact_first_name: Option<String>,
    #[serde(default)]
    pub contact_last_name: Option<String>,
    #[serde(default)]
    pub contact_email: Option<String>,
    #[serde(default)]
    pub contact_phone: Option<String>,
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
}

/// CSV shape for the Access Groups import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessGroupImportRow {
    /// Required. The access group name to create.
    pub group_name: String,
    #[serde(default)]
    pub group_description: Option<String>,
    /// "ORG_UNIT" (default) or "DEVICE".
    #[serde(default)]
    pub group_type: Option<String>,
    /// Optional customer name where this group lives. Empty/missing = SO level.
    #[serde(default)]
    pub customer_name: Option<String>,
    /// Semicolon-separated list of customer/site names whose org units this group covers.
    /// If empty for an SO-level group, all customers under the SO are used.
    #[serde(default)]
    pub org_unit_names: Option<String>,
    /// "true" / "false". Defaults to true.
    #[serde(default)]
    pub auto_include_new_org_units: Option<String>,
}

/// CSV shape for the User Roles import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRoleImportRow {
    /// Required. The role name to create.
    pub role_name: String,
    #[serde(default)]
    pub role_description: Option<String>,
    /// Optional customer name where this role lives. Empty/missing = SO level.
    #[serde(default)]
    pub customer_name: Option<String>,
    /// Semicolon-separated list of permission names. Resolved against rolePermissionIds.csv.
    #[serde(default)]
    pub permissions: Option<String>,
}

/// CSV shape for the Users import.
///
/// N-central uses the email address as the login name — there is no separate
/// username field — so this row exposes only `email` and the importer passes it
/// to SOAP as both `<ei2:username>` and the `email` setting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserImportRow {
    /// Required. Email address — also used as the N-central login name.
    pub email: String,
    /// Required. First name.
    pub first_name: String,
    /// Required. Last name.
    pub last_name: String,
    /// Required. Service org / customer / site name where the user will be placed.
    /// Empty / "SO" / "Service Org" / the connected SO's name → SO level.
    pub customer_name: String,
    /// Semicolon-separated list of role names to assign.
    #[serde(default)]
    pub role_names: Option<String>,
    /// Semicolon-separated list of access group names to assign.
    #[serde(default)]
    pub access_group_names: Option<String>,
    #[serde(default = "default_true", deserialize_with = "deserialize_lenient_bool")]
    pub is_enabled: bool,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub department: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Lenient bool deserializer for CSV cells: accepts empty (-> true), case-insensitive
/// true/false/yes/no/1/0, and trims whitespace. Empty defaults to true so blank
/// `isEnabled` columns in templates don't blow up the parse.
fn deserialize_lenient_bool<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let raw = Option::<String>::deserialize(deserializer)?;
    let s = raw.unwrap_or_default();
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(true);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "true" | "yes" | "y" | "1" | "t" => Ok(true),
        "false" | "no" | "n" | "0" | "f" => Ok(false),
        other => Err(D::Error::custom(format!(
            "expected true/false (or yes/no, 1/0), got `{}`",
            other
        ))),
    }
}

/// Resource types that this importer can handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportResource {
    Customers,
    Sites,
    AccessGroups,
    UserRoles,
    Users,
}

impl ImportResource {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "customers" => Some(Self::Customers),
            "sites" => Some(Self::Sites),
            "access_groups" => Some(Self::AccessGroups),
            "user_roles" => Some(Self::UserRoles),
            "users" => Some(Self::Users),
            _ => None,
        }
    }
}

/// Write a CSV template (header + one example row) for the given resource.
pub fn write_template(resource: ImportResource, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut writer = csv::Writer::from_writer(file);

    match resource {
        ImportResource::Customers => writer
            .serialize(CustomerImportRow {
                customer_name: "Acme Co".into(),
                external_id: Some("ACME-001".into()),
                contact_first_name: Some("Jane".into()),
                contact_last_name: Some("Doe".into()),
                contact_email: Some("jane@acme.example".into()),
                contact_phone: Some("555-0100".into()),
                street1: Some("123 Main St".into()),
                street2: None,
                city: Some("Springfield".into()),
                state_prov: Some("IL".into()),
                country: Some("US".into()),
                postal_code: Some("62701".into()),
            })
            .map_err(|e| AppError::Export(format!("CSV write error: {}", e)))?,
        ImportResource::Sites => writer
            .serialize(SiteImportRow {
                customer_name: "Acme Co".into(),
                site_name: "Acme HQ".into(),
                external_id: Some("ACME-HQ".into()),
                contact_first_name: Some("Jane".into()),
                contact_last_name: Some("Doe".into()),
                contact_email: Some("jane@acme.example".into()),
                contact_phone: Some("555-0100".into()),
                street1: Some("123 Main St".into()),
                street2: None,
                city: Some("Springfield".into()),
                state_prov: Some("IL".into()),
                country: Some("US".into()),
                postal_code: Some("62701".into()),
            })
            .map_err(|e| AppError::Export(format!("CSV write error: {}", e)))?,
        ImportResource::AccessGroups => writer
            .serialize(AccessGroupImportRow {
                group_name: "Helpdesk Tier 1".into(),
                group_description: Some("First-line support".into()),
                group_type: Some("ORG_UNIT".into()),
                customer_name: None,
                org_unit_names: Some("Acme Co;Beta Industries".into()),
                auto_include_new_org_units: Some("true".into()),
            })
            .map_err(|e| AppError::Export(format!("CSV write error: {}", e)))?,
        ImportResource::UserRoles => writer
            .serialize(UserRoleImportRow {
                role_name: "Desktop Support".into(),
                role_description: Some("Read access plus active issue triage".into()),
                customer_name: None,
                permissions: Some("ACTIVE_ISSUES_VIEW;CUSTOMER_VIEW".into()),
            })
            .map_err(|e| AppError::Export(format!("CSV write error: {}", e)))?,
        ImportResource::Users => writer
            .serialize(UserImportRow {
                email: "jane@acme.example".into(),
                first_name: "Jane".into(),
                last_name: "Doe".into(),
                customer_name: "Acme Co".into(),
                role_names: Some("Desktop Support".into()),
                access_group_names: Some("Helpdesk Tier 1".into()),
                is_enabled: true,
                phone: Some("555-0100".into()),
                department: Some("IT".into()),
                location: Some("HQ".into()),
            })
            .map_err(|e| AppError::Export(format!("CSV write error: {}", e)))?,
    }

    writer
        .flush()
        .map_err(|e| AppError::Export(format!("CSV flush error: {}", e)))?;
    Ok(())
}

/// Parse a CSV file into a typed row vec.
pub fn read_rows<T>(path: &Path) -> Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let file = std::fs::File::open(path)
        .map_err(|e| AppError::Export(format!("Failed to open CSV: {}", e)))?;
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .has_headers(true)
        .from_reader(file);

    let mut rows = Vec::new();
    for (i, result) in reader.deserialize::<T>().enumerate() {
        let row = result
            .map_err(|e| AppError::Export(format!("CSV parse error on row {}: {}", i + 2, e)))?;
        rows.push(row);
    }
    Ok(rows)
}

/// Outcome of importing a single CSV row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowOutcome {
    pub row_number: usize,
    pub status: RowStatus,
    pub label: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RowStatus {
    Created,
    Skipped,
    Error,
    /// Dry-run "would be created"
    Planned,
}
