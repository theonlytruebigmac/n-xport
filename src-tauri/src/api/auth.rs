//! Authentication manager for N-Central API
//!
//! Handles JWT exchange, token refresh, and credential storage.

use crate::error::{ApiError, ApiResult};
use crate::models::{AuthResponse, AuthState, RefreshResponse};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use zeroize::Zeroizing;

/// Manages authentication state and token refresh
pub struct AuthManager {
    /// Current auth state
    state: Arc<RwLock<Option<AuthState>>>,
    /// Base URL for auth endpoints
    base_url: String,
    /// HTTP client for auth requests
    http: reqwest::Client,
    /// The JWT this session was opened with, kept in memory only so a long run can sign in
    /// again once the refresh token expires. It is never logged or written to disk.
    jwt: Arc<RwLock<Option<Zeroizing<String>>>>,
    /// Only one task refreshes or signs in again at a time; the others reuse its result.
    refresh_lock: Arc<Mutex<()>>,
}

impl AuthManager {
    /// Create a new auth manager
    pub fn new(base_url: &str, cookie_store: Arc<reqwest::cookie::Jar>) -> Self {
        Self {
            state: Arc::new(RwLock::new(None)),
            jwt: Arc::new(RwLock::new(None)),
            refresh_lock: Arc::new(Mutex::new(())),
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .cookie_provider(cookie_store)
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Authenticate using a JWT token
    pub async fn authenticate(&self, jwt: &str) -> ApiResult<()> {
        let url = format!("{}/api/auth/authenticate", self.base_url);

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", jwt))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();

            return Err(match status {
                401 | 403 => ApiError::Authentication(message),
                429 => ApiError::RateLimited {
                    retry_after_secs: 60,
                },
                _ => ApiError::Server { status, message },
            });
        }

        // Get response body as text
        let body = response
            .text()
            .await
            .map_err(|e| ApiError::InvalidResponse(e.to_string()))?;

        // Sanitize body for logging
        tracing::debug!("Auth response received, length: {}", body.len());

        // Parse the JSON
        let auth_response: AuthResponse = serde_json::from_str(&body).map_err(|e| {
            ApiError::InvalidResponse(format!(
                "JSON parse error: {}. Body length: {}",
                e,
                body.len()
            ))
        })?;

        let state = AuthState::from_response(auth_response);
        *self.state.write().await = Some(state);
        *self.jwt.write().await = Some(Zeroizing::new(jwt.to_string()));

        Ok(())
    }

    /// Get a valid access token, refreshing if needed
    pub async fn get_token(&self) -> ApiResult<String> {
        // Determine what action to take while holding the read lock,
        // then release the lock before performing any async work that
        // needs a write lock (avoids RwLock deadlock).
        enum Action {
            ReturnToken(String),
            Refresh,
            NotAuthenticated,
            TokenExpired,
        }

        let action = {
            let state = self.state.read().await;
            match &*state {
                None => Action::NotAuthenticated,
                Some(s) if s.is_refresh_expired() => Action::TokenExpired,
                Some(s) if s.is_access_expired() => Action::Refresh,
                Some(s) => Action::ReturnToken(s.access_token.clone()),
            }
            // Read lock dropped here
        };

        match action {
            Action::ReturnToken(token) => Ok(token),
            Action::Refresh | Action::TokenExpired => self.renew_token().await,
            Action::NotAuthenticated => Err(ApiError::Authentication("Not authenticated".into())),
        }
    }

    /// Get a new access token: refresh it, or sign in again with the JWT when the refresh
    /// token has expired or is refused. Concurrent callers share one renewal.
    async fn renew_token(&self) -> ApiResult<String> {
        let _guard = self.refresh_lock.lock().await;

        // Another task may have renewed the token while this one waited for the lock.
        let refresh_token = {
            let state = self.state.read().await;
            match &*state {
                Some(s) if !s.is_access_expired() => return Ok(s.access_token.clone()),
                Some(s) if !s.is_refresh_expired() => Some(s.refresh_token.clone()),
                Some(_) => None,
                None => return Err(ApiError::Authentication("Not authenticated".into())),
            }
        };

        if let Some(refresh_token) = refresh_token {
            match self.refresh_token_internal(&refresh_token).await {
                Ok(token) => return Ok(token),
                Err(ApiError::TokenExpired) => {}
                Err(other) => return Err(other),
            }
        }

        let jwt = self.jwt.read().await.as_ref().map(|jwt| jwt.to_string());
        let Some(jwt) = jwt else {
            return Err(ApiError::TokenExpired);
        };
        tracing::info!("Refresh token expired; signing in again with the session JWT");
        self.authenticate(&jwt).await?;
        let state = self.state.read().await;
        state
            .as_ref()
            .map(|s| s.access_token.clone())
            .ok_or_else(|| ApiError::Authentication("Not authenticated".into()))
    }

    /// Refresh the access token using the refresh token
    #[allow(dead_code)]
    async fn refresh_token(&self) -> ApiResult<String> {
        let refresh_token = {
            let state = self.state.read().await;
            match &*state {
                Some(s) => s.refresh_token.clone(),
                None => return Err(ApiError::Authentication("No refresh token".into())),
            }
        };

        self.refresh_token_internal(&refresh_token).await
    }

    /// Internal refresh implementation with token passed in
    async fn refresh_token_internal(&self, refresh_token: &str) -> ApiResult<String> {
        let url = format!("{}/api/auth/refresh", self.base_url);

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", refresh_token))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            if status == 401 || status == 403 {
                return Err(ApiError::TokenExpired);
            }
            let message = response.text().await.unwrap_or_default();
            return Err(ApiError::Server { status, message });
        }

        let refresh_response: RefreshResponse = response
            .json()
            .await
            .map_err(|e| ApiError::InvalidResponse(e.to_string()))?;

        // Update the access token
        let mut state = self.state.write().await;
        if let Some(ref mut s) = *state {
            let now = chrono::Utc::now();
            s.access_token = refresh_response.tokens.access.token.clone();
            s.access_expires_at =
                now + chrono::Duration::seconds(refresh_response.tokens.access.expires_in_seconds);
        }

        Ok(refresh_response.tokens.access.token)
    }

    /// Check if we're currently authenticated
    pub async fn is_authenticated(&self) -> bool {
        let state = self.state.read().await;
        match &*state {
            Some(s) => !s.is_refresh_expired(),
            None => false,
        }
    }

    /// Clear authentication state
    pub async fn logout(&self) {
        *self.state.write().await = None;
        *self.jwt.write().await = None;
    }

    /// Get current auth state for debugging
    pub async fn get_state(&self) -> Option<AuthState> {
        self.state.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{start, MockServer};
    use chrono::{Duration, Utc};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn state(access_in: i64, refresh_in: i64) -> AuthState {
        AuthState {
            access_token: "old-access".into(),
            refresh_token: "old-refresh".into(),
            access_expires_at: Utc::now() + Duration::seconds(access_in),
            refresh_expires_at: Utc::now() + Duration::seconds(refresh_in),
        }
    }

    async fn manager(server: &MockServer, initial: AuthState) -> AuthManager {
        let manager = AuthManager::new(&server.base_url, Arc::new(reqwest::cookie::Jar::default()));
        *manager.state.write().await = Some(initial);
        *manager.jwt.write().await = Some(Zeroizing::new("session-jwt".into()));
        manager
    }

    #[tokio::test]
    async fn a_valid_token_is_returned_without_any_request() {
        let server = start(|_, _, _| unreachable!("no request expected")).await;
        let manager = manager(&server, state(3600, 7200)).await;
        assert_eq!(manager.get_token().await.unwrap(), "old-access");
        assert_eq!(server.requests().len(), 0);
    }

    #[tokio::test]
    async fn an_expired_access_token_is_refreshed() {
        let server = start(|_, path, _| {
            assert_eq!(path, "/api/auth/refresh");
            (
                200,
                r#"{"tokens":{"access":{"token":"refreshed","expiresInSeconds":3600}}}"#.into(),
            )
        })
        .await;
        let manager = manager(&server, state(-10, 7200)).await;
        assert_eq!(manager.get_token().await.unwrap(), "refreshed");
        assert_eq!(manager.get_token().await.unwrap(), "refreshed");
        assert_eq!(server.count("POST", "/api/auth/refresh"), 1);
    }

    #[tokio::test]
    async fn an_expired_refresh_token_signs_in_again_with_the_session_jwt() {
        // The mock answers /api/auth/authenticate itself with the token "test-access".
        let server = start(|_, path, _| panic!("unexpected request to {path}")).await;
        let manager = manager(&server, state(-10, -10)).await;
        assert_eq!(manager.get_token().await.unwrap(), "test-access");
        assert_eq!(server.requests().len(), 0, "no refresh was attempted");
    }

    #[tokio::test]
    async fn a_refused_refresh_signs_in_again_too() {
        let server = start(|_, path, _| {
            assert_eq!(path, "/api/auth/refresh");
            (401, r#"{"message":"refresh token revoked"}"#.into())
        })
        .await;
        let manager = manager(&server, state(-10, 7200)).await;
        assert_eq!(manager.get_token().await.unwrap(), "test-access");
    }

    #[tokio::test]
    async fn without_a_session_jwt_an_expired_session_is_reported() {
        let server = start(|_, _, _| unreachable!()).await;
        let manager = manager(&server, state(-10, -10)).await;
        manager.logout().await;
        *manager.state.write().await = Some(state(-10, -10));
        assert!(matches!(
            manager.get_token().await,
            Err(ApiError::TokenExpired)
        ));
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_renewal() {
        let refreshes = Arc::new(AtomicUsize::new(0));
        let counter = refreshes.clone();
        let server = start(move |_, _, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            (
                200,
                r#"{"tokens":{"access":{"token":"refreshed","expiresInSeconds":3600}}}"#.into(),
            )
        })
        .await;
        let manager = Arc::new(manager(&server, state(-10, 7200)).await);
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let manager = manager.clone();
                tokio::spawn(async move { manager.get_token().await.unwrap() })
            })
            .collect();
        for task in tasks {
            assert_eq!(task.await.unwrap(), "refreshed");
        }
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    }
}
