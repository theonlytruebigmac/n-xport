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
                if let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
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
    let (base_url, _, server) = mock_server(vec![(401, r#"{"message":"denied"}"#, None)]).await;
    let client = NcClient::new(&base_url);
    assert!(matches!(
        client.authenticate("test-jwt").await,
        Err(ApiError::Authentication(_))
    ));
    server.await.expect("mock server task");

    let (base_url, _, server) = mock_server(vec![(200, r#"{"tokens":{}}"#, None)]).await;
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
        (
            429,
            r#"{"message":"slow down"}"#,
            Some("Retry-After: 0\r\n"),
        ),
        (
            429,
            r#"{"message":"slow down"}"#,
            Some("Retry-After: 0\r\n"),
        ),
        (
            429,
            r#"{"message":"slow down"}"#,
            Some("Retry-After: 0\r\n"),
        ),
        (
            429,
            r#"{"message":"slow down"}"#,
            Some("Retry-After: 0\r\n"),
        ),
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
    let (base_url, requests, server) = mock_server(vec![
        (200, AUTH_RESPONSE, None),
        (200, "{}", None),
        (200, "{}", None),
    ])
    .await;
    let client = NcClient::new(&base_url);
    authenticate(&client).await;

    client
        .update_org_unit_property_value(31, 92, "sample value")
        .await
        .expect("property update should succeed");
    client
        .update_device_property_value(88, 92, "device property value")
        .await
        .expect("device property update should succeed");

    server.await.expect("mock server task");
    let requests = requests.lock().expect("request lock");
    assert!(requests[1].starts_with("PUT /api/org-units/31/custom-properties/92 HTTP/1.1"));
    assert!(requests[1].contains(r#"{"value":"sample value"}"#));
    assert!(requests[2].starts_with("PUT /api/devices/88/custom-properties/92 HTTP/1.1"));
    assert!(requests[2].contains(r#"{"value":"device property value"}"#));
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

mod paging_behaviour {
    use crate::test_support::{authenticated_client, json_response, start_with_query, MockServer};

    /// How a fake server reports and honours paging.
    #[derive(Clone, Copy)]
    struct Server {
        items: usize,
        /// The most items the server will return per page, whatever is asked for.
        max_page_size: usize,
        report_total_items: bool,
        report_total_pages: bool,
        /// Ignore the page number and always answer with the first page.
        ignore_paging: bool,
    }

    fn param(query: &str, name: &str) -> Option<usize> {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{}=", name)))
            .and_then(|value| value.parse().ok())
    }

    async fn serve(config: Server) -> MockServer {
        start_with_query(move |_, _, query, _| {
            let requested = param(query, "pageSize").unwrap_or(config.max_page_size);
            let size = requested.min(config.max_page_size).max(1);
            let page = if config.ignore_paging {
                1
            } else {
                param(query, "pageNumber").unwrap_or(1)
            };
            let start = ((page - 1) * size).min(config.items);
            let end = (start + size).min(config.items);
            let mut body = serde_json::json!({ "data": (start..end).collect::<Vec<_>>() });
            if config.report_total_items {
                body["totalItems"] = config.items.into();
            }
            if config.report_total_pages {
                body["totalPages"] = config.items.div_ceil(size).into();
            }
            json_response(body)
        })
        .await
    }

    async fn fetch_all(config: Server, requested_page_size: u32) -> (Vec<usize>, usize) {
        let server = serve(config).await;
        let client = authenticated_client(&server).await;
        let items: Vec<usize> = client
            .get_all_pages("/api/test", requested_page_size, |_, _| {})
            .await
            .expect("paged fetch");
        (items, server.count("GET", "/api/test"))
    }

    fn base(items: usize) -> Server {
        Server {
            items,
            max_page_size: 1000,
            report_total_items: true,
            report_total_pages: true,
            ignore_paging: false,
        }
    }

    fn all(items: usize) -> Vec<usize> {
        (0..items).collect()
    }

    #[tokio::test]
    async fn gets_every_page_when_the_server_reports_everything() {
        let (items, requests) = fetch_all(base(250), 100).await;
        assert_eq!(items, all(250));
        assert_eq!(requests, 3);
    }

    #[tokio::test]
    async fn gets_every_page_when_the_server_caps_the_page_size_below_the_request() {
        // Asked for 100 per page, the server gives 50. With only `totalItems`, the page count
        // must not be worked out from the requested size.
        for (total_items, total_pages) in [(true, false), (true, true), (false, true)] {
            let config = Server {
                max_page_size: 50,
                report_total_items: total_items,
                report_total_pages: total_pages,
                ..base(130)
            };
            let (items, _) = fetch_all(config, 100).await;
            assert_eq!(
                items,
                all(130),
                "totalItems={total_items} totalPages={total_pages}"
            );
        }
    }

    #[tokio::test]
    async fn gets_every_page_with_no_totals_even_when_the_server_caps_the_page_size() {
        let config = Server {
            max_page_size: 50,
            report_total_items: false,
            report_total_pages: false,
            ..base(130)
        };
        let (items, _) = fetch_all(config, 100).await;
        assert_eq!(items, all(130));
    }

    #[tokio::test]
    async fn handles_a_total_that_is_an_exact_multiple_of_the_page_size() {
        let (items, requests) = fetch_all(base(200), 100).await;
        assert_eq!(items, all(200));
        assert_eq!(
            requests, 2,
            "no request past the last full page when totals are known"
        );

        let no_totals = Server {
            report_total_items: false,
            report_total_pages: false,
            ..base(200)
        };
        let (items, _) = fetch_all(no_totals, 100).await;
        assert_eq!(items, all(200));
    }

    #[tokio::test]
    async fn handles_empty_and_single_item_results() {
        assert_eq!(fetch_all(base(0), 100).await, (vec![], 1));
        assert_eq!(fetch_all(base(1), 100).await, (vec![0], 1));
    }

    #[tokio::test]
    async fn stops_when_the_server_ignores_paging_instead_of_looping() {
        // Endpoints that are not paginated answer every request with the full list.
        for (total_items, total_pages) in [(false, false), (true, false)] {
            let config = Server {
                report_total_items: total_items,
                report_total_pages: total_pages,
                ignore_paging: true,
                ..base(7)
            };
            let (items, requests) = fetch_all(config, 5).await;
            assert_eq!(items, all(5));
            assert!(requests <= 3, "stopped after {requests} requests");
        }
    }
}

#[tokio::test]
async fn a_dropped_connection_on_a_read_is_retried_but_a_write_is_not() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let dropped = Arc::new(AtomicUsize::new(0));
    let seen = dropped.clone();
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0u8; 8192];
            let count = stream.read(&mut buffer).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buffer[..count]).into_owned();
            let line = head.lines().next().unwrap_or_default().to_string();
            let body = if line.contains("/api/auth/authenticate") {
                AUTH_RESPONSE.to_string()
            } else if line.starts_with("GET /api/flaky") && seen.fetch_add(1, Ordering::SeqCst) == 0
            {
                continue; // drop the connection without answering
            } else if line.starts_with("POST /api/writes") {
                seen.fetch_add(100, Ordering::SeqCst);
                continue;
            } else {
                r#"{"data":[1,2,3],"totalItems":3}"#.to_string()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });

    let client = NcClient::new(&format!("http://{}", address));
    authenticate(&client).await;

    let items: Vec<i32> = client
        .get_all_pages("/api/flaky", 100, |_, _| {})
        .await
        .expect("the read succeeds after one retry");
    assert_eq!(items, vec![1, 2, 3]);
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        2,
        "one dropped attempt, one success"
    );

    let write: Result<serde_json::Value, _> = client
        .post("/api/writes", &serde_json::json!({"name": "x"}))
        .await;
    assert!(write.is_err());
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        102,
        "the write was attempted exactly once"
    );
    server.abort();
}
