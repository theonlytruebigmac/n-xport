//! Registration token export: one row per customer and site.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::stream::{self, StreamExt};

use crate::api::client::NcClient;
use crate::error::ApiResult;
use crate::models::{Customer, RegistrationToken, RegistrationTokenRow, Site};

/// A customer or site whose registration token is to be fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenTarget {
    pub org_unit_id: i64,
    pub org_unit_type: &'static str,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub site_id: Option<i64>,
    pub site_name: Option<String>,
}

impl TokenTarget {
    pub(crate) fn describe(&self) -> String {
        match (&self.site_name, &self.customer_name) {
            (Some(site), Some(customer)) => format!("site {} ({})", site, customer),
            (Some(site), None) => format!("site {}", site),
            (None, _) => format!(
                "customer {}",
                self.customer_name.as_deref().unwrap_or("(unnamed)")
            ),
        }
    }
}

/// Customers first, then each site with the customer it belongs to, ordered by name so the
/// output is stable. Service organizations are left out: N-central only issues tokens for
/// customers and sites.
pub(crate) fn token_targets(customers: &[Customer], sites: &[Site]) -> Vec<TokenTarget> {
    let names: HashMap<i64, &str> = customers
        .iter()
        .map(|c| (c.customer_id, c.customer_name.as_str()))
        .collect();

    let mut customer_targets: Vec<TokenTarget> = customers
        .iter()
        .map(|customer| TokenTarget {
            org_unit_id: customer.customer_id,
            org_unit_type: "customer",
            customer_id: Some(customer.customer_id),
            customer_name: Some(customer.customer_name.clone()),
            site_id: None,
            site_name: None,
        })
        .collect();
    customer_targets
        .sort_by(|a, b| (&a.customer_name, a.org_unit_id).cmp(&(&b.customer_name, b.org_unit_id)));

    let mut site_targets: Vec<TokenTarget> = sites
        .iter()
        .map(|site| {
            let customer_id = site.customer_id.or(site.customerid).or(site.parent_id);
            let customer_name = customer_id
                .and_then(|id| names.get(&id))
                .map(|name| name.to_string());
            TokenTarget {
                org_unit_id: site.site_id,
                org_unit_type: "site",
                customer_id: customer_name.as_ref().and(customer_id),
                customer_name,
                site_id: Some(site.site_id),
                site_name: Some(site.site_name.clone()),
            }
        })
        .collect();
    // Sites whose customer is unknown go last.
    site_targets.sort_by_key(|t| {
        (
            t.customer_name.is_none(),
            t.customer_name.clone(),
            t.site_name.clone(),
            t.org_unit_id,
        )
    });

    customer_targets.into_iter().chain(site_targets).collect()
}

pub(crate) fn token_row(target: &TokenTarget, token: RegistrationToken) -> RegistrationTokenRow {
    RegistrationTokenRow {
        org_unit_id: target.org_unit_id,
        org_unit_type: target.org_unit_type.to_string(),
        customer_id: target.customer_id,
        customer_name: target.customer_name.clone(),
        site_id: target.site_id,
        site_name: target.site_name.clone(),
        registration_token: token.registration_token,
        registration_token_expiry_date: token.registration_token_expiry_date,
    }
}

/// Where a token fetch reports its progress on the shared progress bar.
#[derive(Clone, Copy)]
pub(crate) struct TokenPhase<'a> {
    pub name: &'a str,
    pub base_percent: f32,
    pub percent_range: f32,
}

/// Fetch the token of each target, five at a time. Results come back in the order of
/// `targets`; targets skipped because of cancellation are missing.
pub(crate) async fn fetch_tokens(
    client: &NcClient,
    targets: Vec<TokenTarget>,
    phase: TokenPhase<'_>,
    emit_progress: &(dyn Fn(&str, &str, f32) + Send + Sync),
    cancel_token: &AtomicBool,
) -> Vec<(TokenTarget, ApiResult<RegistrationToken>)> {
    let total = targets.len();
    emit_progress(
        phase.name,
        &format!("Fetching tokens for {} customers and sites...", total),
        phase.base_percent,
    );

    let mut results: Vec<_> = stream::iter(targets.into_iter().enumerate())
        .map(|(index, target)| async move {
            if cancel_token.load(Ordering::Relaxed) {
                return None;
            }
            let token = client.get_registration_token(target.org_unit_id).await;
            Some((index, target, token))
        })
        .buffer_unordered(5)
        .filter_map(|result| async move { result })
        .collect()
        .await;
    results.sort_by_key(|(index, _, _)| *index);

    results
        .into_iter()
        .map(|(index, target, token)| {
            if index % 10 == 0 {
                emit_progress(
                    phase.name,
                    &format!("Fetched {}/{}", index, total),
                    phase.base_percent + (index as f32 / total.max(1) as f32) * phase.percent_range,
                );
            }
            (target, token)
        })
        .collect()
}

/// Fetch the token of every customer and site. A failure for one org unit becomes a warning
/// and the rest are still exported.
pub(super) async fn fetch_registration_token_rows(
    client: &NcClient,
    customers: &[Customer],
    sites: &[Site],
    emit_progress: &(dyn Fn(&str, &str, f32) + Send + Sync),
    warnings: &mut Vec<String>,
    cancel_token: &AtomicBool,
) -> Vec<RegistrationTokenRow> {
    let phase = TokenPhase {
        name: "Registration Tokens",
        base_percent: 94.0,
        percent_range: 5.0,
    };
    let targets = token_targets(customers, sites);
    let mut rows = Vec::with_capacity(targets.len());
    for (target, token) in fetch_tokens(client, targets, phase, emit_progress, cancel_token).await {
        match token {
            Ok(token) => rows.push(token_row(&target, token)),
            Err(e) => warnings.push(format!(
                "Failed to fetch the registration token for {}: {}",
                target.describe(),
                e
            )),
        }
    }
    rows
}
