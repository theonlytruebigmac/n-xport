//! Site imports.

use super::common::extract_id;
use super::context::ImportContext;
use crate::api::NcClient;
use crate::import::{RowOutcome, RowStatus, SiteImportRow};
use serde_json::json;
use tauri::{AppHandle, Runtime};

pub async fn import_site<R: Runtime>(
    row_number: usize,
    row: SiteImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    dry_run: bool,
    _app: &AppHandle<R>,
) -> RowOutcome {
    let site_name = row.site_name.trim();
    let cust_name = row.customer_name.trim();
    let label = format!("{} / {}", cust_name, site_name);

    if site_name.is_empty() || cust_name.is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message: "customerName and siteName are required".into(),
        };
    }

    let cust_key = cust_name.to_lowercase();
    let site_key = site_name.to_lowercase();

    let cust_id = match ctx.customers_by_name.get(&cust_key) {
        Some(&id) => id,
        None => {
            return RowOutcome {
                row_number,
                status: RowStatus::Error,
                label,
                message: format!(
                    "Parent customer '{}' not found under SO {}",
                    cust_name, ctx.source_so_id
                ),
            }
        }
    };

    if let Some(&existing_id) = ctx.sites_by_name.get(&(cust_key.clone(), site_key.clone())) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label,
            message: format!("Site already exists (ID: {})", existing_id),
        };
    }
    let unconfirmed_key = format!("site:{}/{}", cust_key, site_key);
    if ctx.unconfirmed_creates.contains(&unconfirmed_key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label,
            message: "Site was already created earlier in this session (server returned no ID)"
                .into(),
        };
    }

    if dry_run {
        return RowOutcome {
            row_number,
            status: RowStatus::Planned,
            label,
            message: format!("Would create site under customer ID {}", cust_id),
        };
    }

    let payload = json!({
        "siteName": site_name,
        "externalId": row.external_id,
        "contactFirstName": row.contact_first_name,
        "contactLastName": row.contact_last_name,
        "contactEmail": row.contact_email,
        "contactPhone": row.contact_phone,
        "contactPhoneExt": row.contact_phone_ext,
        "phone": row.phone,
        "contactTitle": row.contact_title,
        "contactDepartment": row.contact_department,
        "street1": row.street1,
        "street2": row.street2,
        "city": row.city,
        "stateProv": row.state_prov,
        "country": row.country,
        "postalCode": row.postal_code,
    });

    match client.create_site(cust_id, &payload).await {
        Ok(resp) => {
            let id = extract_id(&resp, &["siteId", "id"]);
            if id != 0 {
                ctx.sites_by_name.insert((cust_key, site_key), id);
            } else {
                ctx.unconfirmed_creates.insert(unconfirmed_key);
            }
            RowOutcome {
                row_number,
                status: RowStatus::Created,
                label,
                message: if id != 0 {
                    format!("Created site (ID: {})", id)
                } else {
                    "Created (no ID returned)".into()
                },
            }
        }
        Err(e) => RowOutcome {
            row_number,
            status: RowStatus::Error,
            label,
            message: format!("Failed: {}", e),
        },
    }
}
