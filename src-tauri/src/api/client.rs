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

#[cfg(test)]
mod tests {
            use super::NcClient;
            use crate::error::ApiError;
            use std::sync::{Arc, Mutex};
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            use tokio::net::TcpListener;

            const AUTH_RESPONSE: &str = r#"{"tokens":{"access":{"token":"test-access","expiresInSeconds":3600},"refresh":{"token":"test-refresh","expiresInSeconds":3600}}}"#;

            async fn mock_server(
                responses: Vec<(u16, &'static str, Option<&'static str>)>,
            ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
                let listener = TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind mock server");
                let address = listener.local_addr().expect("mock server address");
                let requests = Arc::new(Mutex::new(Vec::new()));
                let recorded_requests = requests.clone();
                let task = tokio::spawn(async move {
                    for (status, body, extra_header) in responses {
                        let (mut stream, _) = listener.accept().await.expect("accept request");
                        let mut request = Vec::new();
                        let mut buffer = [0; 1024];
                        loop {
                            let count = stream.read(&mut buffer).await.expect("read request");
                            if count == 0 {
                                break;
                            }
                            request.extend_from_slice(&buffer[..count]);
                            if let Some(header_end) = request
                                .windows(4)
                                .position(|window| window == b"\r\n\r\n")
                            {
                                let header_text = String::from_utf8_lossy(&request[..header_end]);
                                let content_length = header_text
                                    .lines()
                                    .find_map(|line| {
                                        let (name, value) = line.split_once(':')?;
                                        name.eq_ignore_ascii_case("content-length")
                                            .then(|| value.trim().parse::<usize>().ok())
                                            .flatten()
                                    })
                                    .unwrap_or(0);
                                if request.len() >= header_end + 4 + content_length {
                                    break;
                                }
                            }
                        }
                        let request = String::from_utf8_lossy(&request).into_owned();
                        let request_line = request.lines().next().unwrap_or_default().to_string();
                        recorded_requests
                            .lock()
                            .expect("request lock")
                            .push(format!("{}\n{}", request_line, request));

                        let reason = match status {
                            200 => "OK",
                            201 => "Created",
                            401 => "Unauthorized",
                            404 => "Not Found",
                            429 => "Too Many Requests",
                            _ => "Error",
                        };
                        let header = extra_header.unwrap_or("");
                        let response = format!(
                            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                            status,
                            reason,
                            body.len(),
                            header,
                            body
                        );
                        stream
                            .write_all(response.as_bytes())
                            .await
                            .expect("write response");
                    }
                });
                (format!("http://{}", address), requests, task)
            }

            async fn authenticate(client: &NcClient) {
                client
                    .authenticate("test-jwt")
                    .await
                    .expect("mock authentication should succeed");
            }

            #[tokio::test]
            async fn authenticates_and_fetches_all_pages_for_supported_metadata_shapes() {
                let cases = [
                    (
                        "totalItems only",
                        vec![
                            (200, r#"{"data":[1,2],"totalItems":3}"#, None),
                            (200, r#"{"data":[3],"totalItems":3}"#, None),
                        ],
                    ),
                    (
                        "totalPages only",
                        vec![
                            (200, r#"{"data":[1,2],"totalPages":2}"#, None),
                            (200, r#"{"data":[3],"totalPages":2}"#, None),
                        ],
                    ),
                    (
                        "both totals",
                        vec![
                            (
                                200,
                                r#"{"data":[1,2],"pageNumber":1,"pageSize":2,"totalItems":3,"totalPages":2}"#,
                                None,
                            ),
                            (
                                200,
                                r#"{"data":[3],"pageNumber":2,"pageSize":2,"totalItems":3,"totalPages":2}"#,
                                None,
                            ),
                        ],
                    ),
                    (
                        "no metadata with final partial page",
                        vec![
                            (200, r#"{"data":[1,2]}"#, None),
                            (200, r#"{"data":[3]}"#, None),
                        ],
                    ),
                    (
                        "empty final page",
                        vec![
                            (200, r#"{"data":[1,2]}"#, None),
                            (200, r#"{"data":[]}"#, None),
                        ],
                    ),
                    (
                        "reported total with final partial page",
                        vec![
                            (200, r#"{"data":[1,2],"totalItems":3}"#, None),
                            (200, r#"{"data":[3],"totalItems":3}"#, None),
                        ],
                    ),
                ];

                for (name, pages) in cases {
                    let mut responses = vec![(200, AUTH_RESPONSE, None)];
                    responses.extend(pages.iter().copied());
                    let (base_url, requests, server) = mock_server(responses).await;
                    let client = NcClient::new(&base_url);
                    authenticate(&client).await;

                    let items: Vec<i32> = client
                        .get_all_pages("/api/test", 2, |_, _| {})
                        .await
                        .unwrap_or_else(|error| panic!("{} failed: {}", name, error));
                    let expected_items = if name == "empty final page" {
                        vec![1, 2]
                    } else {
                        vec![1, 2, 3]
                    };
                    assert_eq!(items, expected_items, "case: {}", name);

                    server.await.expect("mock server task");
                    let requests = requests.lock().expect("request lock");
                    assert_eq!(requests.len(), pages.len() + 1, "case: {}", name);
                    assert!(
                        requests[0]
                            .to_ascii_lowercase()
                            .contains("authorization: bearer test-jwt"),
                        "JWT must be sent using the documented bearer scheme"
                    );
                    assert!(requests[1].starts_with("GET /api/test?pageNumber=1&pageSize=2"));
                    if pages.len() > 1 {
                        assert!(requests[2].starts_with("GET /api/test?pageNumber=2&pageSize=2"));
                    }
                }
            }

            #[tokio::test]
            async fn returns_authentication_and_not_found_errors() {
                let (base_url, _, server) =
                    mock_server(vec![(401, r#"{"message":"denied"}"#, None)]).await;
                let client = NcClient::new(&base_url);
                assert!(matches!(
                    client.authenticate("test-jwt").await,
                    Err(ApiError::Authentication(_))
                ));
                server.await.expect("mock server task");

                let (base_url, _, server) =
                    mock_server(vec![(200, r#"{"tokens":{}}"#, None)]).await;
                let client = NcClient::new(&base_url);
                assert!(matches!(
                    client.authenticate("test-jwt").await,
                    Err(ApiError::InvalidResponse(_))
                ));
                server.await.expect("mock server task");

                let (base_url, _, server) = mock_server(vec![
                    (200, AUTH_RESPONSE, None),
                    (404, r#"{"message":"missing"}"#, None),
                ])
                .await;
                let client = NcClient::new(&base_url);
                authenticate(&client).await;
                assert!(matches!(
                    client.get_service_org_by_id(12).await,
                    Err(ApiError::NotFound(_))
                ));
                server.await.expect("mock server task");
            }

            #[tokio::test]
            async fn retries_rate_limited_request_then_returns_rate_limit_error() {
                let responses = vec![
                    (200, AUTH_RESPONSE, None),
                    (429, r#"{"message":"slow down"}"#, Some("Retry-After: 0\r\n")),
                    (429, r#"{"message":"slow down"}"#, Some("Retry-After: 0\r\n")),
                    (429, r#"{"message":"slow down"}"#, Some("Retry-After: 0\r\n")),
                    (429, r#"{"message":"slow down"}"#, Some("Retry-After: 0\r\n")),
                ];
                let (base_url, requests, server) = mock_server(responses).await;
                let client = NcClient::new(&base_url);
                authenticate(&client).await;

                assert!(matches!(
                    client.get_service_org_by_id(12).await,
                    Err(ApiError::RateLimited { .. })
                ));
                server.await.expect("mock server task");
                assert_eq!(requests.lock().expect("request lock").len(), 5);
            }

            #[tokio::test]
            async fn sends_documented_custom_property_update_path_and_body() {
                let (base_url, requests, server) =
                    mock_server(vec![(200, AUTH_RESPONSE, None), (200, "{}", None)]).await;
                let client = NcClient::new(&base_url);
                authenticate(&client).await;

                client
                    .update_org_unit_property_value(31, 92, "sample value")
                    .await
                    .expect("property update should succeed");

                server.await.expect("mock server task");
                let requests = requests.lock().expect("request lock");
                assert!(requests[1].starts_with(
                    "PUT /api/org-units/31/custom-properties/92 HTTP/1.1"
                ));
                assert!(requests[1].contains(r#"{"value":"sample value"}"#));
            }

            #[tokio::test]
            async fn sends_create_requests_with_openapi_routes_and_payload_shapes() {
                let (base_url, requests, server) = mock_server(vec![
                    (200, AUTH_RESPONSE, None),
                    (201, r#"{"customerId":101}"#, None),
                    (201, r#"{"siteId":201}"#, None),
                    (201, r#"{"roleId":301}"#, None),
                    (201, r#"{"groupId":401}"#, None),
                    (201, r#"{"groupId":501}"#, None),
                ])
                .await;
                let client = NcClient::new(&base_url);
                authenticate(&client).await;

                client
                    .create_customer(
                        10,
                        &serde_json::json!({
                            "customerName": "Acme",
                            "externalId": "ACME-1",
                            "contactPhoneExt": "42"
                        }),
                    )
                    .await
                    .expect("customer create should succeed");
                client
                    .create_site(
                        101,
                        &serde_json::json!({
                            "siteName": "HQ",
                            "contactDepartment": "IT"
                        }),
                    )
                    .await
                    .expect("site create should succeed");
                client
                    .create_user_role(
                        10,
                        &serde_json::json!({
                            "roleName": "Support",
                            "description": "Support role",
                            "permissionIds": ["1701", "1702"],
                            "userIds": []
                        }),
                    )
                    .await
                    .expect("role create should succeed");
                client
                    .create_org_unit_access_group(
                        10,
                        &serde_json::json!({
                            "groupName": "Operators",
                            "groupDescription": "Operator group",
                            "orgUnitIds": ["101"],
                            "userIds": ["201"],
                            "autoIncludeNewOrgUnits": "true"
                        }),
                    )
                    .await
                    .expect("organization access group create should succeed");
                client
                    .create_device_access_group(
                        10,
                        &serde_json::json!({
                            "groupName": "Device operators",
                            "groupDescription": "Device group",
                            "deviceIds": ["301"],
                            "userIds": ["201"]
                        }),
                    )
                    .await
                    .expect("device access group create should succeed");

                server.await.expect("mock server task");
                let requests = requests.lock().expect("request lock");
                assert_eq!(requests.len(), 6);

                let assert_request = |index: usize, method_path: &str, expected_body: serde_json::Value| {
                    let request = &requests[index];
                    assert!(request.starts_with(method_path), "{request}");
                    let body = request
                        .split_once("\r\n\r\n")
                        .expect("request headers and body")
                        .1;
                    let actual_body: serde_json::Value =
                        serde_json::from_str(body).expect("request body should be JSON");
                    assert_eq!(actual_body, expected_body);
                };

                assert_request(
                    1,
                    "POST /api/service-orgs/10/customers HTTP/1.1",
                    serde_json::json!({
                        "customerName": "Acme",
                        "externalId": "ACME-1",
                        "contactPhoneExt": "42"
                    }),
                );
                assert_request(
                    2,
                    "POST /api/customers/101/sites HTTP/1.1",
                    serde_json::json!({"siteName": "HQ", "contactDepartment": "IT"}),
                );
                assert_request(
                    3,
                    "POST /api/org-units/10/user-roles HTTP/1.1",
                    serde_json::json!({
                        "roleName": "Support",
                        "description": "Support role",
                        "permissionIds": ["1701", "1702"],
                        "userIds": []
                    }),
                );
                assert_request(
                    4,
                    "POST /api/org-units/10/access-groups HTTP/1.1",
                    serde_json::json!({
                        "groupName": "Operators",
                        "groupDescription": "Operator group",
                        "orgUnitIds": ["101"],
                        "userIds": ["201"],
                        "autoIncludeNewOrgUnits": "true"
                    }),
                );
                assert_request(
                    5,
                    "POST /api/org-units/10/device-access-groups HTTP/1.1",
                    serde_json::json!({
                        "groupName": "Device operators",
                        "groupDescription": "Device group",
                        "deviceIds": ["301"],
                        "userIds": ["201"]
                    }),
                );
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
            let parse_text = if body_text.is_empty() { "{}" } else { &body_text };

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
        let mut page: u32 = 1;

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
            let page_info = response.page_info.as_ref();
            let reported_page_size = page_info
                .and_then(|info| info.page_size)
                .filter(|size| *size > 0)
                .unwrap_or(page_size);
            let total_pages = page_info.and_then(|info| {
                info.total_pages.or_else(|| {
                    info.total_items.map(|total_items| {
                        total_items.div_ceil(reported_page_size)
                    })
                })
            });
            on_progress(page, total_pages.unwrap_or(page));
            all_items.extend(response.data);

            // Safety: always break if no items returned
            if count == 0 {
                break;
            }

            if total_pages.is_some_and(|total| page >= total) {
                break;
            }

            let has_more_by_total_items = page_info
                .and_then(|info| info.total_items)
                .is_some_and(|total_items| {
                    u64::from(page) * u64::from(reported_page_size) < u64::from(total_items)
                });
            if total_pages.is_none()
                && page_info.and_then(|info| info.total_items).is_none()
                && (count as u32) < page_size
            {
                tracing::info!(
                    "Received partial page ({} < {}), assuming end of data",
                    count,
                    page_size
                );
                break;
            }

            if page_info.and_then(|info| info.total_items).is_some()
                && !has_more_by_total_items
            {
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
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {}).await
    }

    /// Get all sites
    pub async fn get_sites(&self) -> ApiResult<Vec<Site>> {
        self.get_all_pages(paths::SITES, DEFAULT_PAGE_SIZE, |_, _| {}).await
    }

    /// Get all sites (global list, used by export with client-side SO filtering)
    pub async fn get_sites_by_so(&self, so_id: i64) -> ApiResult<Vec<Site>> {
        let path = endpoints::service_org_sites(so_id);
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {}).await
    }

    /// Get sites directly under a specific customer via /api/customers/{id}/sites
    /// Used by migration to scope site fetches to each mapped customer.
    pub async fn get_sites_by_customer(&self, customer_id: i64) -> ApiResult<Vec<Site>> {
        let path = endpoints::customer_sites(customer_id);
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {}).await
    }

    /// Get all devices
    pub async fn get_devices(&self) -> ApiResult<Vec<Device>> {
        self.get_all_pages(paths::DEVICES, DEFAULT_PAGE_SIZE, |_, _| {}).await
    }

    /// Get users for an org unit
    pub async fn get_users_by_org_unit(&self, org_unit_id: i64) -> ApiResult<Vec<User>> {
        let path = endpoints::org_unit_users(org_unit_id);
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {}).await
    }

    /// Get devices under an org unit (service org)
    pub async fn get_devices_by_org_unit(&self, org_unit_id: i64) -> ApiResult<Vec<Device>> {
        let path = endpoints::org_unit_devices(org_unit_id);
        self.get_all_pages(&path, DEFAULT_PAGE_SIZE, |_, _| {}).await
    }

    /// Get access groups for an org unit
    pub async fn get_access_groups(&self, org_unit_id: i64) -> ApiResult<Vec<AccessGroup>> {
        let path = endpoints::org_unit_access_groups(org_unit_id);
        self.get_all_pages(&path, 100, |_, _| {}).await
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
}
