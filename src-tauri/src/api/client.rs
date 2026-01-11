//! N-Central REST API client
//!
//! Provides async methods for all N-Central API endpoints with
//! automatic rate limiting, pagination, and token refresh.

use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use super::auth::AuthManager;
use super::endpoints::{self, paths, PaginationParams};
use super::rate_limiter::RateLimiter;
use crate::error::{ApiError, ApiResult};
use crate::models::*;

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

    /// Authenticate with JWT
    pub async fn authenticate(&self, jwt: &str) -> ApiResult<()> {
        self.auth.authenticate(jwt).await
    }

    /// Check if authenticated
    pub async fn is_authenticated(&self) -> bool {
        self.auth.is_authenticated().await
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

            let response = self
                .http
                .request(method.clone(), &url)
                .query(query)
                .json(body)
                .bearer_auth(&token)
                .send()
                .await?;

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
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(5);

                tracing::warn!("Rate limited, retrying after {} seconds", retry_after);
                sleep(Duration::from_secs(retry_after)).await;
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

            // Parse JSON, logging body on error
            return serde_json::from_str(&body_text).map_err(|e| {
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

    /// Fetch all pages of a paginated endpoint
    pub async fn get_all_pages<T, F>(
        &self,
        path: &str,
        page_size: u32,
        mut on_progress: F,
    ) -> ApiResult<Vec<T>>
    where
        T: DeserializeOwned,
        F: FnMut(u32, u32),
    {
        let mut all_items = Vec::new();
        let mut page = 1;

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
            all_items.extend(response.data);

            // Report progress
            if let Some(ref page_info) = response.page_info {
                on_progress(page, page_info.total_pages);

                if page >= page_info.total_pages {
                    break;
                }
            }

            // Safety: always break if no items returned
            if count == 0 {
                break;
            }

            // Safety: if we got fewer items than requested page size, this matches the last page
            if (count as u32) < page_size {
                tracing::info!(
                    "Received partial page ({} < {}), assuming end of data",
                    count,
                    page_size
                );
                break;
            }

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
        let response: PaginatedResponse<ServiceOrg> = self.get(paths::SERVICE_ORGS).await?;
        Ok(response.data)
    }

    /// Get a single service organization by ID
    pub async fn get_service_org_by_id(&self, so_id: i64) -> ApiResult<ServiceOrg> {
        self.get(&endpoints::service_org_by_id(so_id)).await
    }

    /// Get all customers
    pub async fn get_customers(&self) -> ApiResult<Vec<Customer>> {
        self.get_all_pages(paths::CUSTOMERS, 100, |_, _| {}).await
    }

    /// Get customers under a service org
    pub async fn get_customers_by_so(&self, so_id: i64) -> ApiResult<Vec<Customer>> {
        let path = endpoints::service_org_customers(so_id);
        self.get_all_pages(&path, 100, |_, _| {}).await
    }

    /// Get all sites
    pub async fn get_sites(&self) -> ApiResult<Vec<Site>> {
        self.get_all_pages(paths::SITES, 100, |_, _| {}).await
    }

    /// Get sites under a service org
    pub async fn get_sites_by_so(&self, so_id: i64) -> ApiResult<Vec<Site>> {
        let path = endpoints::service_org_sites(so_id);
        self.get_all_pages(&path, 100, |_, _| {}).await
    }

    /// Get all devices
    pub async fn get_devices(&self) -> ApiResult<Vec<Device>> {
        self.get_all_pages(paths::DEVICES, 100, |_, _| {}).await
    }

    /// Get users for an org unit
    pub async fn get_users_by_org_unit(&self, org_unit_id: i64) -> ApiResult<Vec<User>> {
        let path = endpoints::org_unit_users(org_unit_id);
        self.get_all_pages(&path, 100, |_, _| {}).await
    }

    /// Get devices under an org unit (service org)
    pub async fn get_devices_by_org_unit(&self, org_unit_id: i64) -> ApiResult<Vec<Device>> {
        let path = endpoints::org_unit_devices(org_unit_id);
        self.get_all_pages(&path, 100, |_, _| {}).await
    }

    /// Get access groups for an org unit
    pub async fn get_access_groups(&self, org_unit_id: i64) -> ApiResult<Vec<AccessGroup>> {
        let path = endpoints::org_unit_access_groups(org_unit_id);
        let response: PaginatedResponse<AccessGroup> = self.get(&path).await?;
        Ok(response.data)
    }

    /// Get user roles for an org unit
    pub async fn get_user_roles(&self, org_unit_id: i64) -> ApiResult<Vec<UserRole>> {
        let path = endpoints::org_unit_user_roles(org_unit_id);
        let response: PaginatedResponse<UserRole> = self.get(&path).await?;
        Ok(response.data)
    }

    /// Get custom properties for an org unit
    pub async fn get_org_properties(&self, org_unit_id: i64) -> ApiResult<Vec<OrgProperty>> {
        let path = endpoints::org_unit_custom_properties(org_unit_id);
        let response: PaginatedResponse<OrgProperty> = self.get(&path).await?;
        Ok(response.data)
    }

    /// Get custom properties for a device
    pub async fn get_device_properties(&self, device_id: i64) -> ApiResult<Vec<DeviceProperty>> {
        let path = endpoints::device_custom_properties(device_id);
        let response: PaginatedResponse<DeviceProperty> = self.get(&path).await?;
        Ok(response.data)
    }

    /// Get device by ID
    pub async fn get_device(&self, device_id: i64) -> ApiResult<Device> {
        let path = endpoints::device_by_id(device_id);
        self.get(&path).await
    }

    /// Get device assets
    pub async fn get_device_assets(&self, device_id: i64) -> ApiResult<DeviceAsset> {
        let path = endpoints::device_assets(device_id);
        self.get(&path).await
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

    /// Create a user
    pub async fn create_user(
        &self,
        org_unit_id: i64,
        user: &serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        let path = endpoints::org_unit_users(org_unit_id);
        self.post(&path, user).await
    }

    /// Set a custom property value
    pub async fn set_custom_property_value(
        &self,
        value_data: &serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        self.post(paths::CUSTOM_PROPERTIES_VALUES, value_data).await
    }
}
