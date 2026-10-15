//! Customers and sites, and matching existing destination org units when customers are not migrated.

use super::env::{emit_log, fetch_or_warn, items_until_cancelled, report_progress, MigrationEnv};
use super::summary::MigrationSummary;
use super::types::IdMapping;
use crate::models::*;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{Emitter, Runtime};

/// Migrate customers and sites from source to destination.
/// Populates `mapping.customers`, `mapping.sites`, and `mapping.org_units`.
pub(super) async fn migrate_customers_and_sites<R: Runtime>(
    env: &MigrationEnv<'_, R>,
    mapping: &mut IdMapping,
    summary: &mut MigrationSummary,
) -> Result<(), String> {
    let MigrationEnv {
        source,
        dest,
        source_so_id,
        dest_so_id,
        soap: soap_client,
        app_handle,
        cancel_token,
        ..
    } = *env;
    if cancel_token.load(Ordering::Relaxed) {
        return Ok(());
    }
    report_progress(
        app_handle,
        "Customers",
        "Fetching source customers...",
        10.0,
    );
    let source_customers = source
        .get_customers_by_so(source_so_id)
        .await
        .map_err(|e| format!("Failed to fetch source customers: {}", e))?;
    if cancel_token.load(Ordering::Relaxed) {
        return Ok(());
    }

    report_progress(
        app_handle,
        "Customers",
        "Fetching destination customers...",
        15.0,
    );
    let dest_customers = dest
        .get_customers_by_so(dest_so_id)
        .await
        .map_err(|e| format!("Failed to fetch destination customers: {}", e))?;
    if cancel_token.load(Ordering::Relaxed) {
        return Ok(());
    }

    let mut dest_name_map_raw = HashMap::new();
    for c in &dest_customers {
        dest_name_map_raw.insert(c.customer_name.to_lowercase(), c.customer_id);
    }
    let dest_name_map = std::sync::Arc::new(dest_name_map_raw);

    // Build source customer id -> name map for later site mapping
    let source_cust_id_to_name: HashMap<i64, String> = source_customers
        .iter()
        .map(|c| (c.customer_id, c.customer_name.clone()))
        .collect();

    let total = source_customers.len();

    use futures::stream::{self, StreamExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let completed = Arc::new(AtomicUsize::new(0));

    let bodies = stream::iter(source_customers)
        .map(|source_cust| {
            let dest = dest.clone();
            let dest_name_map = dest_name_map.clone();
            let completed = completed.clone();
            let app_handle = app_handle.clone();
            let cancel_token = cancel_token.clone();

            async move {
                if cancel_token.load(Ordering::Relaxed) {
                    return (source_cust.customer_id, 0, None, None);
                }
                let (dest_id, failure, skipped) = if let Some(&id) =
                    dest_name_map.get(&source_cust.customer_name.to_lowercase())
                {
                    let msg = format!(
                        "Customer '{}' already exists on destination (ID: {})",
                        source_cust.customer_name, id
                    );
                    tracing::info!("{}", msg);
                    emit_log(&app_handle, "debug", &msg);
                    (
                        id,
                        None,
                        Some(format!(
                            "Customer '{}' already exists (ID: {})",
                            source_cust.customer_name, id
                        )),
                    )
                } else {
                    let payload = serde_json::json!({
                        "customerName": source_cust.customer_name,
                        "externalId": source_cust.external_id,
                        "contactFirstName": source_cust.contact_first_name,
                        "contactLastName": source_cust.contact_last_name,
                        "contactEmail": source_cust.contact_email,
                        "contactPhone": source_cust.contact_phone,
                        "contactPhoneExt": source_cust.contact_phone_ext,
                        "phone": source_cust.phone,
                        "contactTitle": source_cust.contact_title,
                        "contactDepartment": source_cust.contact_department,
                        "street1": source_cust.street1,
                        "street2": source_cust.street2,
                        "city": source_cust.city,
                        "stateProv": source_cust.state_prov,
                        "country": source_cust.country,
                        "postalCode": source_cust.postal_code,
                    });

                    match dest.create_customer(dest_so_id, &payload).await {
                        Ok(resp) => {
                            let data = if resp.get("data").is_some() { &resp["data"] } else { &resp };
                            let id = data["customerId"]
                                .as_i64()
                                .or_else(|| data["id"].as_i64())
                                .or_else(|| resp["customerId"].as_i64())
                                .or_else(|| resp["id"].as_i64())
                                .unwrap_or(0);
                            if id > 0 {
                                let msg = format!(
                                    "Created customer '{}' (ID: {})",
                                    source_cust.customer_name, id
                                );
                                tracing::info!("{}", msg);
                                emit_log(&app_handle, "success", &msg);
                            } else {
                                let msg = format!(
                                    "Customer '{}' created but no ID returned in response: {:?}",
                                    source_cust.customer_name, resp
                                );
                                tracing::warn!("{}", msg);
                                emit_log(&app_handle, "warning", &msg);
                            }
                            if id <= 0 {
                                (0, Some(format!("Customer '{}' created without a destination ID", source_cust.customer_name)), None)
                            } else {
                                (id, None, None)
                            }
                        }
                        Err(rest_err) => {
                            if cancel_token.load(Ordering::Relaxed) {
                                return (
                                    source_cust.customer_id,
                                    0,
                                    Some(format!(
                                        "Customer '{}' REST creation failed before cancellation: {}",
                                        source_cust.customer_name, rest_err
                                    )),
                                    None,
                                );
                            }
                            // REST failed — try SOAP fallback
                            tracing::warn!("REST create_customer failed for '{}': {}. Trying SOAP fallback...", source_cust.customer_name, rest_err);
                            if let Some(soap) = soap_client {
                                match soap.customer_add(
                                    &source_cust.customer_name,
                                    dest_so_id,
                                    source_cust.external_id.as_deref(),
                                    source_cust.contact_first_name.as_deref(),
                                    source_cust.contact_last_name.as_deref(),
                                    source_cust.contact_email.as_deref(),
                                ).await {
                                    Ok(id) => {
                                        if id > 0 {
                                            let msg = format!("Created customer '{}' via SOAP (ID: {})", source_cust.customer_name, id);
                                            tracing::info!("{}", msg);
                                            emit_log(&app_handle, "success", &msg);
                                            (id, None, None)
                                        } else {
                                            (0, Some(format!(
                                                "Customer '{}' SOAP creation returned invalid ID {}",
                                                source_cust.customer_name, id
                                            )), None)
                                        }
                                    }
                                    Err(soap_err) => {
                                        let msg = format!("Failed to create customer '{}' (REST: {}, SOAP: {})", source_cust.customer_name, rest_err, soap_err);
                                        tracing::error!("{}", msg);
                                        emit_log(&app_handle, "error", &msg);
                                        (0, Some(format!("Customer '{}' failed: REST: {}; SOAP: {}", source_cust.customer_name, rest_err, soap_err)), None)
                                    }
                                }
                            } else {
                                let msg = format!("Failed to create customer '{}': {}", source_cust.customer_name, rest_err);
                                tracing::error!("{}", msg);
                                emit_log(&app_handle, "error", &msg);
                                (0, Some(format!("Customer '{}' failed: {}", source_cust.customer_name, rest_err)), None)
                            }
                        }
                    }
                };

                let count = completed.fetch_add(1, Ordering::SeqCst) + 1;
                let progress = 15.0 + (count as f32 / total as f32) * 15.0;
                let _ = app_handle.emit(
                    "export-progress",
                    ProgressUpdate {
                        phase: "Customers".to_string(),
                        message: format!("Processed customer: {}", source_cust.customer_name),
                        percent: progress,
                        current: count as u32,
                        total: total as u32,
                    },
                );

                (source_cust.customer_id, dest_id, failure, skipped)
            }
        })
        .buffer_unordered(2);

    let results: Vec<(i64, i64, Option<String>, Option<String>)> = bodies.collect().await;

    for (source_id, dest_id, failure, skipped) in results {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        if dest_id > 0 {
            mapping.customers.insert(source_id, dest_id);
            mapping.org_units.insert(source_id, dest_id);
        } else if let Some(failure) = failure {
            summary.customers_failed.push(failure);
        }
        if let Some(skipped) = skipped {
            summary.skipped.push(skipped);
        }
    }

    // Map the Service Org IDs as well
    mapping.org_units.insert(source_so_id, dest_so_id);

    // --- Sites ---
    report_progress(app_handle, "Sites", "Fetching source sites...", 30.0);
    let fetch_failures_before_source_sites = summary.fetch_failures.len();
    if cancel_token.load(Ordering::Relaxed) {
        return Ok(());
    }
    let source_sites = fetch_or_warn(
        source.get_sites_by_so(source_so_id).await,
        app_handle,
        &format!("source sites for SO {}", source_so_id),
        &mut summary.fetch_failures,
    );
    if cancel_token.load(Ordering::Relaxed)
        || summary.fetch_failures.len() != fetch_failures_before_source_sites
    {
        return Ok(());
    }
    // `/api/sites` lists every site on the server, not just this service org's. Sites
    // under other service orgs' customers are not part of this migration, so they are
    // left out rather than reported as failures. A site with no parent at all is kept
    // so it is still reported.
    let source_sites: Vec<_> = source_sites
        .into_iter()
        .filter(|site| {
            site.parent_id
                .or(site.customer_id)
                .or(site.customerid)
                .is_none_or(|parent| source_cust_id_to_name.contains_key(&parent))
        })
        .collect();
    let fetch_failures_before_dest_sites = summary.fetch_failures.len();
    let dest_sites = fetch_or_warn(
        dest.get_sites_by_so(dest_so_id).await,
        app_handle,
        &format!("dest sites for SO {}", dest_so_id),
        &mut summary.fetch_failures,
    );
    if summary.fetch_failures.len() != fetch_failures_before_dest_sites {
        return Ok(());
    }

    // Build dest site lookup: (parent_customer_name, site_name) -> site_id
    let mut dest_site_lookup: HashMap<(String, String), i64> = HashMap::new();
    for site in &dest_sites {
        if cancel_token.load(Ordering::Relaxed) {
            return Ok(());
        }
        let parent_id = site.parent_id.or(site.customer_id).or(site.customerid);
        if let Some(pid) = parent_id {
            if let Some(dest_cust) = dest_customers.iter().find(|c| c.customer_id == pid) {
                dest_site_lookup.insert(
                    (
                        dest_cust.customer_name.to_lowercase(),
                        site.site_name.to_lowercase(),
                    ),
                    site.site_id,
                );
            }
        }
    }

    for source_site in items_until_cancelled(source_sites, cancel_token) {
        let source_parent_id = source_site
            .parent_id
            .or(source_site.customer_id)
            .or(source_site.customerid);
        if let Some(src_parent_id) = source_parent_id {
            if let Some(src_cust_name) = source_cust_id_to_name.get(&src_parent_id) {
                let key = (
                    src_cust_name.to_lowercase(),
                    source_site.site_name.to_lowercase(),
                );
                if let Some(&dest_site_id) = dest_site_lookup.get(&key) {
                    mapping.sites.insert(source_site.site_id, dest_site_id);
                    mapping.org_units.insert(source_site.site_id, dest_site_id);
                    let msg = format!(
                        "Site '{}' under '{}' already exists on destination (ID: {})",
                        source_site.site_name, src_cust_name, dest_site_id
                    );
                    tracing::debug!("{}", msg);
                    emit_log(app_handle, "debug", &msg);
                    summary.skipped.push(msg);
                } else if let Some(&dest_cust_id) = mapping.customers.get(&src_parent_id) {
                    let msg = format!(
                        "Creating site '{}' under customer '{}' (dest customer ID: {})...",
                        source_site.site_name, src_cust_name, dest_cust_id
                    );
                    tracing::info!("{}", msg);
                    emit_log(app_handle, "info", &msg);

                    let payload = serde_json::json!({
                        "siteName": source_site.site_name,
                        "externalId": source_site.external_id,
                        "contactFirstName": source_site.contact_first_name,
                        "contactLastName": source_site.contact_last_name,
                        "contactEmail": source_site.contact_email,
                        "contactPhone": source_site.contact_phone,
                        "contactPhoneExt": source_site.contact_phone_ext,
                        "phone": source_site.phone,
                        "contactTitle": source_site.contact_title,
                        "contactDepartment": source_site.contact_department,
                        "street1": source_site.street1,
                        "street2": source_site.street2,
                        "city": source_site.city,
                        "stateProv": source_site.state_prov,
                        "country": source_site.country,
                        "postalCode": source_site.postal_code,
                    });

                    match dest.create_site(dest_cust_id, &payload).await {
                        Ok(resp) => {
                            let data = if resp.get("data").is_some() {
                                &resp["data"]
                            } else {
                                &resp
                            };
                            let id = data["siteId"]
                                .as_i64()
                                .or_else(|| data["id"].as_i64())
                                .or_else(|| resp["siteId"].as_i64())
                                .or_else(|| resp["id"].as_i64())
                                .unwrap_or(0);
                            if id > 0 {
                                // Remember the new site so a second source site with the
                                // same name maps to it instead of being created again.
                                dest_site_lookup.insert(key, id);
                                mapping.sites.insert(source_site.site_id, id);
                                mapping.org_units.insert(source_site.site_id, id);
                                let msg = format!(
                                    "Created site '{}' (ID: {})",
                                    source_site.site_name, id
                                );
                                tracing::info!("{}", msg);
                                emit_log(app_handle, "success", &msg);
                            } else {
                                summary.sites_failed.push(format!(
                                    "Site '{}' created without a destination ID",
                                    source_site.site_name
                                ));
                                let msg = format!(
                                    "Site '{}' created but no ID returned in response: {:?}",
                                    source_site.site_name, resp
                                );
                                tracing::warn!("{}", msg);
                                emit_log(app_handle, "warning", &msg);
                            }
                        }
                        Err(e) => {
                            summary
                                .sites_failed
                                .push(format!("Site '{}' failed: {}", source_site.site_name, e));
                            let msg =
                                format!("Failed to create site '{}': {}", source_site.site_name, e);
                            tracing::error!("{}", msg);
                            emit_log(app_handle, "error", &msg);
                        }
                    }
                } else {
                    summary.sites_failed.push(format!(
                        "Site '{}' skipped because its parent customer is not mapped",
                        source_site.site_name
                    ));
                    let msg = format!("Site '{}' under customer '{}' skipped - parent customer not mapped to destination", source_site.site_name, src_cust_name);
                    tracing::warn!("{}", msg);
                    emit_log(app_handle, "warning", &msg);
                }
            } else {
                summary.sites_failed.push(format!(
                    "Site '{}' has an unmapped source parent customer {}",
                    source_site.site_name, src_parent_id
                ));
            }
        } else {
            summary.sites_failed.push(format!(
                "Site '{}' has no source parent customer ID",
                source_site.site_name
            ));
        }
    }

    let msg = format!(
        "Org unit mapping: {} customers, {} sites, {} total org_units",
        mapping.customers.len(),
        mapping.sites.len(),
        mapping.org_units.len()
    );
    tracing::info!("{}", msg);
    emit_log(app_handle, "info", &msg);

    Ok(())
}

pub(super) async fn map_existing_org_units<R: Runtime>(
    env: &MigrationEnv<'_, R>,
    mapping: &mut IdMapping,
    summary: &mut MigrationSummary,
) -> Result<(), String> {
    let MigrationEnv {
        source,
        dest,
        source_so_id,
        dest_so_id,
        app_handle,
        cancel_token,
        ..
    } = *env;
    mapping.org_units.insert(source_so_id, dest_so_id);
    let source_customers = match source.get_customers_by_so(source_so_id).await {
        Ok(customers) => customers,
        Err(error) => {
            let message = format!(
                "Failed to fetch source customers for property mapping: {}",
                error
            );
            summary.fetch_failures.push(message.clone());
            emit_log(app_handle, "warning", &message);
            return Ok(());
        }
    };
    let destination_customers = match dest.get_customers_by_so(dest_so_id).await {
        Ok(customers) => customers,
        Err(error) => {
            let message = format!(
                "Failed to fetch destination customers for property mapping: {}",
                error
            );
            summary.fetch_failures.push(message.clone());
            emit_log(app_handle, "warning", &message);
            return Ok(());
        }
    };
    if cancel_token.load(Ordering::Relaxed) {
        return Ok(());
    }

    let mut destination_customers_by_name: HashMap<String, Vec<i64>> = HashMap::new();
    for customer in destination_customers {
        destination_customers_by_name
            .entry(customer.customer_name.trim().to_lowercase())
            .or_default()
            .push(customer.customer_id);
    }

    for source_customer in source_customers {
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        let matches = destination_customers_by_name
            .get(&source_customer.customer_name.trim().to_lowercase())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let dest_customer_id = match matches {
            [customer_id] => *customer_id,
            [] => {
                summary.additional_manual_follow_up.push(format!(
                    "Customer '{}' is not present on destination; customer- and site-level items under it were not migrated",
                    source_customer.customer_name
                ));
                continue;
            }
            _ => {
                summary.additional_manual_follow_up.push(format!(
                    "Customer '{}' is ambiguous on destination; customer- and site-level items under it were not migrated",
                    source_customer.customer_name
                ));
                continue;
            }
        };
        mapping
            .customers
            .insert(source_customer.customer_id, dest_customer_id);
        mapping
            .org_units
            .insert(source_customer.customer_id, dest_customer_id);

        let source_sites = match source
            .get_sites_by_customer(source_customer.customer_id)
            .await
        {
            Ok(sites) => sites,
            Err(error) => {
                let message = format!(
                    "Failed to fetch source sites for customer '{}': {}",
                    source_customer.customer_name, error
                );
                summary.fetch_failures.push(message.clone());
                emit_log(app_handle, "warning", &message);
                continue;
            }
        };
        if cancel_token.load(Ordering::Relaxed) {
            break;
        }
        let destination_sites = match dest.get_sites_by_customer(dest_customer_id).await {
            Ok(sites) => sites,
            Err(error) => {
                let message = format!(
                    "Failed to fetch destination sites for customer '{}': {}",
                    source_customer.customer_name, error
                );
                summary.fetch_failures.push(message.clone());
                emit_log(app_handle, "warning", &message);
                continue;
            }
        };
        let mut destination_sites_by_name: HashMap<String, Vec<i64>> = HashMap::new();
        for site in destination_sites {
            destination_sites_by_name
                .entry(site.site_name.trim().to_lowercase())
                .or_default()
                .push(site.site_id);
        }
        for source_site in source_sites {
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }
            let matches = destination_sites_by_name
                .get(&source_site.site_name.trim().to_lowercase())
                .map(Vec::as_slice)
                .unwrap_or_default();
            match matches {
                [site_id] => {
                    mapping.sites.insert(source_site.site_id, *site_id);
                    mapping.org_units.insert(source_site.site_id, *site_id);
                }
                [] => summary.additional_manual_follow_up.push(format!(
                    "Site '{}' under customer '{}' is not present on destination; customer- and site-level items under it were not migrated",
                    source_site.site_name, source_customer.customer_name
                )),
                _ => summary.additional_manual_follow_up.push(format!(
                    "Site '{}' under customer '{}' is ambiguous on destination; customer- and site-level items under it were not migrated",
                    source_site.site_name, source_customer.customer_name
                )),
            }
        }
    }
    Ok(())
}
