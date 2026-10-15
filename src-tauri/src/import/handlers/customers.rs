//! Customer imports.

use super::common::{emit_log, extract_id};
use super::context::ImportContext;
use crate::api::{NcClient, NcSoapClient};
use crate::import::{CustomerImportRow, RowOutcome, RowStatus};
use serde_json::json;
use tauri::{AppHandle, Runtime};

pub async fn import_customer<R: Runtime>(
    row_number: usize,
    row: CustomerImportRow,
    ctx: &mut ImportContext,
    client: &NcClient,
    soap: Option<&NcSoapClient>,
    dry_run: bool,
    app: &AppHandle<R>,
) -> RowOutcome {
    let name = row.customer_name.trim();
    if name.is_empty() {
        return RowOutcome {
            row_number,
            status: RowStatus::Error,
            label: format!("(row {})", row_number),
            message: "customerName is required".into(),
        };
    }
    let key = name.to_lowercase();

    if let Some(&existing_id) = ctx.customers_by_name.get(&key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: name.to_string(),
            message: format!("Customer already exists (ID: {})", existing_id),
        };
    }
    let unconfirmed_key = format!("customer:{}", key);
    if ctx.unconfirmed_creates.contains(&unconfirmed_key) {
        return RowOutcome {
            row_number,
            status: RowStatus::Skipped,
            label: name.to_string(),
            message: "Customer was already created earlier in this session (server returned no ID)"
                .into(),
        };
    }

    if dry_run {
        return RowOutcome {
            row_number,
            status: RowStatus::Planned,
            label: name.to_string(),
            message: format!("Would create customer under SO {}", ctx.source_so_id),
        };
    }

    let payload = json!({
        "customerName": name,
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

    match client.create_customer(ctx.source_so_id, &payload).await {
        Ok(resp) => {
            let id = extract_id(&resp, &["customerId", "id"]);
            if id != 0 {
                ctx.customers_by_name.insert(key, id);
                RowOutcome {
                    row_number,
                    status: RowStatus::Created,
                    label: name.to_string(),
                    message: format!("Created customer (ID: {})", id),
                }
            } else {
                emit_log(
                    app,
                    "warning",
                    &format!("Customer '{}' created but no ID returned", name),
                );
                ctx.unconfirmed_creates.insert(unconfirmed_key);
                RowOutcome {
                    row_number,
                    status: RowStatus::Created,
                    label: name.to_string(),
                    message: "Created (no ID returned)".into(),
                }
            }
        }
        Err(rest_err) => {
            // Try SOAP fallback
            if let Some(s) = soap {
                match s
                    .customer_add(
                        name,
                        ctx.source_so_id,
                        row.external_id.as_deref(),
                        row.contact_first_name.as_deref(),
                        row.contact_last_name.as_deref(),
                        row.contact_email.as_deref(),
                    )
                    .await
                {
                    Ok(id) if id > 0 => {
                        ctx.customers_by_name.insert(key, id);
                        return RowOutcome {
                            row_number,
                            status: RowStatus::Created,
                            label: name.to_string(),
                            message: format!("Created customer via SOAP (ID: {})", id),
                        };
                    }
                    Ok(_) => {}
                    Err(soap_err) => {
                        return RowOutcome {
                            row_number,
                            status: RowStatus::Error,
                            label: name.to_string(),
                            message: format!("Failed (REST: {}, SOAP: {})", rest_err, soap_err),
                        };
                    }
                }
            }
            RowOutcome {
                row_number,
                status: RowStatus::Error,
                label: name.to_string(),
                message: format!("Failed: {}", rest_err),
            }
        }
    }
}
