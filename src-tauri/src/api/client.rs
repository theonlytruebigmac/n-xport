//! N-Central REST API client
//!
//! Provides async methods for all N-Central API endpoints with
//! automatic rate limiting, pagination, and token refresh.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use super::auth::AuthManager;
use super::endpoints::{self, paths, PaginationParams};
use super::rate_limiter::RateLimiter;
use crate::error::{ApiError, ApiResult};
use crate::models::*;

/// Default page size for paginated API requests
const DEFAULT_PAGE_SIZE: u32 = 100;

/// N-Central API client
#[derive(Clone)]
pub struct NcClient {
    /// HTTP client
    http: reqwest::Client,
    /// Base URL (e.g., "https://ncentral.example.com")
    base_url: String,
    /// Authentication manager
    auth: Arc<AuthManager>,
    /// Rate limiter
    rate_limiter: Arc<RateLimiter>,
    /// Max retries for rate limited requests
    max_retries: u32,
}

impl NcClient {
    /// Create a new client
    pub fn new(base_url: &str) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        let cookie_store = Arc::new(reqwest::cookie::Jar::default());
        let auth = Arc::new(AuthManager::new(&base_url, cookie_store.clone()));

        Self {
            http: reqwest::Client::builder()
                .cookie_provider(cookie_store)
                .timeout(Duration::from_secs(60))
                .pool_max_idle_per_host(10)
                .min_tls_version(reqwest::tls::Version::TLS_1_2)
                .build()
                .expect("Failed to create HTTP client"),
            base_url: base_url.clone(),
            auth,
            rate_limiter: Arc::new(RateLimiter::new()),
            max_retries: 3,
        }
    }
}

impl NcClient {
    /// Authenticate with JWT
    pub async fn authenticate(&self, jwt: &str) -> ApiResult<()> {
        self.auth.authenticate(jwt).await
    }

    /// Make a GET request with query parameters
    async fn get_with_query<T, Q>(&self, path: &str, query: &Q) -> ApiResult<T>
    where
        T: DeserializeOwned,
        Q: serde::Serialize + ?Sized,
    {
        self.request_full(reqwest::Method::GET, path, query, &())
            .await
    }

    /// Make a GET request with rate limiting and auth
    async fn get<T: DeserializeOwned>(&self, path: &str) -> ApiResult<T> {
        self.request(reqwest::Method::GET, path, &()).await
    }

    /// Make a POST request with rate limiting and auth
    async fn post<T, B>(&self, path: &str, body: &B) -> ApiResult<T>
    where
        T: DeserializeOwned,
        B: serde::Serialize + ?Sized,
    {
        self.request_full(reqwest::Method::POST, path, &(), body)
            .await
    }

    /// Make a PUT request with rate limiting and auth
    #[allow(dead_code)]
    async fn put<T, B>(&self, path: &str, body: &B) -> ApiResult<T>
    where
        T: DeserializeOwned,
        B: serde::Serialize + ?Sized,
    {
        self.request_full(reqwest::Method::PUT, path, &(), body)
            .await
    }

    /// Make a PATCH request with rate limiting and auth
    #[allow(dead_code)]
    async fn patch<T, B>(&self, path: &str, body: &B) -> ApiResult<T>
    where
        T: DeserializeOwned,
        B: serde::Serialize + ?Sized,
    {
        self.request_full(reqwest::Method::PATCH, path, &(), body)
            .await
    }

    /// Generic request handler
    async fn request<T, B>(&self, method: reqwest::Method, path: &str, body: &B) -> ApiResult<T>
    where
        T: DeserializeOwned,
        B: serde::Serialize + ?Sized,
    {
        self.request_full(method, path, &(), body).await
    }

    /// Full request handler with query and body
    async fn request_full<T, Q, B>(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &Q,
        body: &B,
    ) -> ApiResult<T>
    where
        T: DeserializeOwned,
        Q: serde::Serialize + ?Sized,
        B: serde::Serialize + ?Sized,
    {
        let url = format!("{}{}", self.base_url, path);
        let mut retries = 0;

        loop {
            // Acquire rate limit permit
            let _permit = self.rate_limiter.acquire(path).await;

            // Get auth token
            let token = self.auth.get_token().await?;

            let sent = self
                .http
                .request(method.clone(), &url)
                .query(query)
                .json(body)
                .bearer_auth(&token)
                .send()
                .await;
            let response = match sent {
                Ok(response) => response,
                // A dropped connection or timeout on a long run should not lose the item.
                // Only reads are retried: repeating a create after a timeout could duplicate it.
                Err(e)
                    if method == reqwest::Method::GET
                        && e.status().is_none()
                        && !e.is_builder()
                        && retries < self.max_retries =>
                {
                    let backoff_secs = 1u64 << retries.min(4);
                    tracing::warn!(
                        "Request to {} failed ({}), retrying in {}s (attempt {}/{})",
                        path,
                        e,
                        backoff_secs,
                        retries + 1,
                        self.max_retries
                    );
                    sleep(Duration::from_secs(backoff_secs)).await;
                    retries += 1;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            let status = response.status();

            // Handle rate limiting
            if status.as_u16() == 429 {
                if retries >= self.max_retries {
                    return Err(ApiError::RateLimited {
                        retry_after_secs: 60,
                    });
                }

                let retry_after = response
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(5)
                    // A huge Retry-After must not freeze a run for hours.
                    .min(60);

                tracing::warn!("Rate limited, retrying after {} seconds", retry_after);
                sleep(Duration::from_secs(retry_after)).await;
                retries += 1;
                continue;
            }

            // Handle transient 5xx errors with exponential backoff
            if matches!(status.as_u16(), 500 | 502 | 503 | 504) {
                if retries >= self.max_retries {
                    let message = response.text().await.unwrap_or_default();
                    return Err(ApiError::Server {
                        status: status.as_u16(),
                        message,
                    });
                }

                let backoff_secs = 1u64 << retries.min(4); // 1s, 2s, 4s, 8s, 16s max
                tracing::warn!(
                    "Server error {} for {}, retrying in {}s (attempt {}/{})",
                    status.as_u16(),
                    path,
                    backoff_secs,
                    retries + 1,
                    self.max_retries
                );
                sleep(Duration::from_secs(backoff_secs)).await;
                retries += 1;
                continue;
            }

            if !status.is_success() {
                let message = response.text().await.unwrap_or_default();
                return Err(match status.as_u16() {
                    401 | 403 => ApiError::Authentication(message),
                    404 => ApiError::NotFound(path.to_string()),
                    _ => ApiError::Server {
                        status: status.as_u16(),
                        message,
                    },
                });
            }

            // Get response body as text first for debugging
            let body_text = response.text().await.map_err(|e| {
                ApiError::InvalidResponse(format!("Failed to read response body: {}", e))
            })?;

            // Some N-central endpoints return 2xx with an empty body (e.g., 201 Created for
            // access group creation). Treat empty body as an empty JSON object so callers
            // receive Ok(Value::Object({})) instead of a parse error.
            let parse_text = if body_text.is_empty() {
                "{}"
            } else {
                &body_text
            };

            // Parse JSON, logging body on error
            return serde_json::from_str(parse_text).map_err(|e| {
                tracing::error!(
                    "JSON parse error for {} {}: {}. Body: {}",
                    method,
                    path,
                    e,
                    &body_text[..body_text.len().min(1000)]
                );
                ApiError::InvalidResponse(format!("Failed to parse response: {}", e))
            });
        }
    }

    /// Fetch all pages of a paginated endpoint.
    ///
    /// The server may return fewer items per page than requested (a configured maximum), may
    /// report only some of `totalItems`/`totalPages`, or, for endpoints without pagination,
    /// may ignore the page parameters altogether. So the loop stops only on evidence that the
    /// data is exhausted, never on the requested page size alone:
    /// - an empty page;
    /// - the items collected reaching a reported `totalItems`;
    /// - reaching a reported `totalPages`;
    /// - with no totals at all, a page shorter than the first one, or a page identical to the
    ///   previous one (the server is ignoring the page number).
    pub async fn get_all_pages<T, F>(
        &self,
        path: &str,
        page_size: u32,
        mut on_progress: F,
    ) -> ApiResult<Vec<T>>
    where
        T: DeserializeOwned + Serialize,
        F: FnMut(u32, u32),
    {
        // Far more than any real tenant; only stops a server that never runs out.
        const MAX_PAGES: u32 = 100_000;

        let mut all_items: Vec<T> = Vec::new();
        let mut page: u32 = 1;
        let mut first_page_count: Option<usize> = None;
        let mut previous_page: Option<String> = None;

        loop {
            let params = PaginationParams::new().page(page).page_size(page_size);

            let response: PaginatedResponse<T> = self.get_with_query(path, &params).await?;

            let count = response.data.len();
            tracing::info!(
                "Fetching {}: Page {} got {} items. Page info: {:?}",
                path,
                page,
                count,
                response.page_info
            );
            let reported_total_pages = response.page_info.as_ref().and_then(|i| i.total_pages);
            let total_items = response.page_info.as_ref().and_then(|i| i.total_items);
            let page_fingerprint = serde_json::to_string(&response.data).ok();
            let effective_page_size = *first_page_count.get_or_insert(count);

            let estimated_total_pages = reported_total_pages.or_else(|| {
                total_items.map(|total| total.div_ceil(effective_page_size.max(1) as u32))
            });
            on_progress(page, estimated_total_pages.unwrap_or(page));
            if count > 0 && previous_page.as_ref() == page_fingerprint.as_ref() {
                tracing::warn!(
                    "{} returned the same page twice; the server is ignoring pagination",
                    path
                );
                break;
            }
            all_items.extend(response.data);

            if count == 0 {
                break;
            }
            if total_items.is_some_and(|total| all_items.len() as u64 >= u64::from(total)) {
                break;
            }
            if reported_total_pages.is_some_and(|total| page >= total) {
                break;
            }
            if total_items.is_none()
                && reported_total_pages.is_none()
                && count < effective_page_size
            {
                tracing::info!(
                    "Received a short page ({} < {}) with no totals, assuming end of data",
                    count,
                    effective_page_size
                );
                break;
            }
            if page >= MAX_PAGES {
                tracing::warn!("{}: stopped after {} pages", path, MAX_PAGES);
                break;
            }

            previous_page = page_fingerprint;
            page += 1;
        }

        Ok(all_items)
    }

    // ==================== API Methods ====================

    /// Get server info/version
    pub async fn get_server_info(&self) -> ApiResult<ServerInfo> {
        self.get(paths::SERVER_INFO).await
    }

    /// Get all service organizations
    pub async fn get_service_orgs(&self) -> ApiResult<Vec<ServiceOrg>> {
        self.get_all_pages(paths::SERVICE_ORGS, DEFAULT_PAGE_SIZE, |_, _| {})
            .await
    }

    /// Get a single service organization by ID
    pub async fn get_service_org_by_id(&self, so_id: i64) -> ApiResult<ServiceOrg> {
        self.get(&endpoints::service_org_by_id(so_id)).await
    }

    /// Get customers under a service org
    pub async fn get_customers_by_so(&self, so_id: i64) -> ApiResult<Vec<Customer>> {
        let path = endpoints::service_org_customers(so_id);
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {})
            .await
    }

    /// Get all sites
    pub async fn get_sites(&self) -> ApiResult<Vec<Site>> {
        self.get_all_pages(paths::SITES, DEFAULT_PAGE_SIZE, |_, _| {})
            .await
    }

    /// Get all sites (global list, used by export with client-side SO filtering)
    pub async fn get_sites_by_so(&self, so_id: i64) -> ApiResult<Vec<Site>> {
        let path = endpoints::service_org_sites(so_id);
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {})
            .await
    }

    /// Get sites directly under a specific customer via /api/customers/{id}/sites
    /// Used by migration to scope site fetches to each mapped customer.
    pub async fn get_sites_by_customer(&self, customer_id: i64) -> ApiResult<Vec<Site>> {
        let path = endpoints::customer_sites(customer_id);
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {})
            .await
    }

    /// Get all devices
    pub async fn get_devices(&self) -> ApiResult<Vec<Device>> {
        self.get_all_pages(paths::DEVICES, DEFAULT_PAGE_SIZE, |_, _| {})
            .await
    }

    /// Get users for an org unit
    pub async fn get_users_by_org_unit(&self, org_unit_id: i64) -> ApiResult<Vec<User>> {
        let path = endpoints::org_unit_users(org_unit_id);
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {})
            .await
    }

    /// Get devices under an org unit (service org)
    pub async fn get_devices_by_org_unit(&self, org_unit_id: i64) -> ApiResult<Vec<Device>> {
        let path = endpoints::org_unit_devices(org_unit_id);
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {})
            .await
    }

    /// Get access groups for an org unit
    pub async fn get_access_groups(&self, org_unit_id: i64) -> ApiResult<Vec<AccessGroup>> {
        let path = endpoints::org_unit_access_groups(org_unit_id);
        self.get_all_pages(&path, 100, |_, _| {}).await
    }

    /// Full details of one access group, including the org units it covers and its
    /// members. The per-org-unit list omits both.
    pub async fn get_access_group_details(&self, group_id: i64) -> ApiResult<AccessGroup> {
        let value: serde_json::Value = self.get(&endpoints::access_group_by_id(group_id)).await?;
        let data = value.get("data").cloned().unwrap_or(value);
        serde_json::from_value(data).map_err(|e| {
            ApiError::InvalidResponse(format!("Failed to parse access group {}: {}", group_id, e))
        })
    }

    /// Look up the ID of an access group by name at the org unit it was created at.
    /// Creating a group returns no body, so a new group's ID can only be found this way.
    pub async fn find_access_group_id(
        &self,
        org_unit_id: i64,
        name: &str,
    ) -> ApiResult<Option<i64>> {
        let groups = self.get_access_groups(org_unit_id).await?;
        Ok(groups
            .into_iter()
            .find(|group| {
                group
                    .group_name
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
                    && group.org_unit_id.is_none_or(|ou| ou == org_unit_id)
            })
            .map(|group| group.group_id))
    }

    /// Get user roles for an org unit
    pub async fn get_user_roles(&self, org_unit_id: i64) -> ApiResult<Vec<UserRole>> {
        let path = endpoints::org_unit_user_roles(org_unit_id);
        self.get_all_pages(&path, 100, |_, _| {}).await
    }

    /// Get custom properties for an org unit
    pub async fn get_org_properties(&self, org_unit_id: i64) -> ApiResult<Vec<OrgProperty>> {
        let path = endpoints::org_unit_custom_properties(org_unit_id);
        self.get_all_pages(&path, 100, |_, _| {}).await
    }

    /// Get the agent registration token of a customer or site. N-central rejects other
    /// org unit types (service organizations).
    pub async fn get_registration_token(&self, org_unit_id: i64) -> ApiResult<RegistrationToken> {
        let path = endpoints::org_unit_registration_token(org_unit_id);
        let value: serde_json::Value = self.get(&path).await?;
        RegistrationToken::from_response(value).map_err(|e| {
            crate::error::ApiError::InvalidResponse(format!("registration token response: {}", e))
        })
    }

    /// Get custom properties for a device
    pub async fn get_device_properties(&self, device_id: i64) -> ApiResult<Vec<DeviceProperty>> {
        let path = endpoints::device_custom_properties(device_id);
        self.get_all_pages(&path, 100, |_, _| {}).await
    }

    /// Get device assets
    pub async fn get_device_assets(&self, device_id: i64) -> ApiResult<DeviceAsset> {
        let path = endpoints::device_assets(device_id);
        let mut assets: DeviceAsset = self.get(&path).await?;
        assets.device_id = device_id;
        Ok(assets)
    }

    // ==================== Creation Methods ====================

    /// Create a customer
    pub async fn create_customer(
        &self,
        parent_id: i64,
        customer: &serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        let path = endpoints::service_org_customers(parent_id);
        self.post(&path, customer).await
    }

    /// Create a site under a customer
    pub async fn create_site(
        &self,
        customer_id: i64,
        site: &serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        let path = endpoints::customer_sites(customer_id);
        self.post(&path, site).await
    }

    /// Create a user role
    pub async fn create_user_role(
        &self,
        org_unit_id: i64,
        role: &serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        let path = endpoints::org_unit_user_roles(org_unit_id);
        self.post(&path, role).await
    }

    /// Create an org unit type access group
    /// POST to /api/org-units/{id}/access-groups (same path as GET)
    pub async fn create_org_unit_access_group(
        &self,
        org_unit_id: i64,
        group: &serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        let path = endpoints::org_unit_access_groups(org_unit_id);
        self.post(&path, group).await
    }

    /// Create a device type access group
    pub async fn create_device_access_group(
        &self,
        org_unit_id: i64,
        group: &serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        let path = endpoints::device_access_groups_create(org_unit_id);
        self.post(&path, group).await
    }

    /// Update a single org unit custom property value via PUT
    pub async fn update_org_unit_property_value(
        &self,
        org_unit_id: i64,
        property_id: i64,
        value: &str,
    ) -> ApiResult<serde_json::Value> {
        let path = endpoints::org_unit_custom_property(org_unit_id, property_id);
        let body = serde_json::json!({ "value": value });
        self.put(&path, &body).await
    }

    /// Update a single device custom property value via PUT
    pub async fn update_device_property_value(
        &self,
        device_id: i64,
        property_id: i64,
        value: &str,
    ) -> ApiResult<serde_json::Value> {
        let path = endpoints::device_custom_property(device_id, property_id);
        let body = serde_json::json!({ "value": value });
        self.put(&path, &body).await
    }
}

#[cfg(test)]
mod tests;
