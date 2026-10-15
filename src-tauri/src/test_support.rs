//! Shared helpers for tests that need a stateful fake N-central server.
//!
//! The per-test handler sees every request and answers it, so tests can model
//! server state (for example, a customer that exists after a POST) and then
//! assert on what a second import or migration run does against that state.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::api::NcClient;

const AUTH_RESPONSE: &str = r#"{"tokens":{"access":{"token":"test-access","expiresInSeconds":3600},"refresh":{"token":"test-refresh","expiresInSeconds":3600}}}"#;

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    /// Request path without the query string.
    pub path: String,
    pub body: String,
}

pub struct MockServer {
    pub base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    task: tokio::task::JoinHandle<()>,
}

impl MockServer {
    /// Every recorded request except authentication, in arrival order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("request lock").clone()
    }

    pub fn count(&self, method: &str, path: &str) -> usize {
        self.requests()
            .iter()
            .filter(|request| request.method == method && request.path == path)
            .count()
    }

    pub fn bodies(&self, method: &str, path: &str) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|request| request.method == method && request.path == path)
            .map(|request| request.body)
            .collect()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Start a server whose responses come from `handler(method, path, body)`.
/// Authentication is answered automatically and is not recorded.
pub async fn start(
    handler: impl FnMut(&str, &str, &str) -> (u16, String) + Send + 'static,
) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let address = listener.local_addr().expect("mock server address");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(Mutex::new(handler));
    let recorded = requests.clone();

    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            let recorded = recorded.clone();
            tokio::spawn(async move {
                let Some((method, target, body)) = read_request(&mut stream).await else {
                    return;
                };
                let path = target.split('?').next().unwrap_or_default().to_string();
                let (status, response_body) = if path == "/api/auth/authenticate" {
                    (200, AUTH_RESPONSE.to_string())
                } else {
                    recorded
                        .lock()
                        .expect("request lock")
                        .push(RecordedRequest {
                            method: method.clone(),
                            path: path.clone(),
                            body: body.clone(),
                        });
                    (handler.lock().expect("handler lock"))(&method, &path, &body)
                };
                let reason = match status {
                    200 => "OK",
                    201 => "Created",
                    404 => "Not Found",
                    409 => "Conflict",
                    _ => "Error",
                };
                let response = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    reason,
                    response_body.len(),
                    response_body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });

    MockServer {
        base_url: format!("http://{}", address),
        requests,
        task,
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<(String, String, String)> {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let count = stream.read(&mut buffer).await.ok()?;
        if count == 0 {
            return None;
        }
        request.extend_from_slice(&buffer[..count]);
        if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_text = String::from_utf8_lossy(&request[..header_end]).into_owned();
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
                let mut first_line = header_text.lines().next()?.split_whitespace();
                let method = first_line.next()?.to_string();
                let target = first_line.next()?.to_string();
                let body = String::from_utf8_lossy(&request[header_end + 4..]).into_owned();
                return Some((method, target, body));
            }
        }
    }
}

pub async fn authenticated_client(server: &MockServer) -> NcClient {
    let client = NcClient::new(&server.base_url);
    client
        .authenticate("test-jwt")
        .await
        .expect("mock authentication should succeed");
    client
}

pub fn json_response(value: serde_json::Value) -> (u16, String) {
    (200, value.to_string())
}

pub fn not_found() -> (u16, String) {
    (404, r#"{"message":"not found"}"#.to_string())
}
