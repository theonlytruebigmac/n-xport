//! Authentication manager for N-Central API
//!
//! Handles JWT exchange, token refresh, and credential storage.

use crate::error::{ApiError, ApiResult};
use crate::models::{AuthResponse, AuthState, RefreshResponse};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages authentication state and token refresh
pub struct AuthManager {
    /// Current auth state
    state: Arc<RwLock<Option<AuthState>>>,
    /// Base URL for auth endpoints
    base_url: String,
    /// HTTP client for auth requests
    http: reqwest::Client,
}

impl AuthManager {
    /// Create a new auth manager
    pub fn new(base_url: &str, cookie_store: Arc<reqwest::cookie::Jar>) -> Self {
        Self {
            state: Arc::new(RwLock::new(None)),
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

        Ok(())
    }

    /// Get a valid access token, refreshing if needed
    pub async fn get_token(&self) -> ApiResult<String> {
        // Determine what action to take while holding the read lock,
        // then release the lock before performing any async work that
        // needs a write lock (avoids RwLock deadlock).
        enum Action {
            ReturnToken(String),
            Refresh(String),
            NotAuthenticated,
            TokenExpired,
        }

        let action = {
            let state = self.state.read().await;
            match &*state {
                None => Action::NotAuthenticated,
                Some(s) if s.is_refresh_expired() => Action::TokenExpired,
                Some(s) if s.is_access_expired() => Action::Refresh(s.refresh_token.clone()),
                Some(s) => Action::ReturnToken(s.access_token.clone()),
            }
            // Read lock dropped here
        };

        match action {
            Action::ReturnToken(token) => Ok(token),
            Action::Refresh(refresh_token) => self.refresh_token_internal(&refresh_token).await,
            Action::NotAuthenticated => Err(ApiError::Authentication("Not authenticated".into())),
            Action::TokenExpired => Err(ApiError::TokenExpired),
        }
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
    }

    /// Get current auth state for debugging
    pub async fn get_state(&self) -> Option<AuthState> {
        self.state.read().await.clone()
    }
}
