//! Authentication models

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Request body for JWT authentication
#[derive(Debug, Clone, Serialize)]
pub struct AuthRequest {
    // JWT is sent as Bearer token in header, not in body
}

/// Response from /api/auth/authenticate
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub tokens: AuthTokens,
}

/// Access and refresh tokens
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthTokens {
    pub access: TokenInfo,
    pub refresh: TokenInfo,
}

/// Token with expiration info
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub token: String,
    /// Optional - N-Central may not include this, defaults to 1 hour
    #[serde(default = "default_expiry")]
    pub expires_in_seconds: i64,
    #[serde(rename = "type")]
    pub token_type: Option<String>,
}

fn default_expiry() -> i64 {
    3600 // Default 1 hour if not provided
}

/// Stored authentication state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: DateTime<Utc>,
    pub refresh_expires_at: DateTime<Utc>,
}

impl AuthState {
    /// Create new auth state from API response
    pub fn from_response(response: AuthResponse) -> Self {
        let now = Utc::now();
        Self {
            access_token: response.tokens.access.token,
            refresh_token: response.tokens.refresh.token,
            access_expires_at: now + chrono::Duration::seconds(response.tokens.access.expires_in_seconds),
            refresh_expires_at: now + chrono::Duration::seconds(response.tokens.refresh.expires_in_seconds),
        }
    }

    /// Check if access token is expired (with 30 second buffer)
    pub fn is_access_expired(&self) -> bool {
        Utc::now() >= self.access_expires_at - chrono::Duration::seconds(30)
    }

    /// Check if refresh token is expired
    pub fn is_refresh_expired(&self) -> bool {
        Utc::now() >= self.refresh_expires_at
    }
}

/// Response from /api/auth/refresh
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResponse {
    pub tokens: RefreshTokens,
}

/// Tokens from refresh endpoint
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTokens {
    pub access: TokenInfo,
}

/// Server version info from /api/server-info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// N-Central application version (e.g., "2024.1.x")
    #[serde(default)]
    pub version: Option<String>,
    /// API version
    #[serde(default, alias = "api_version")]
    pub api_version: Option<String>,
    /// Build number
    #[serde(default)]
    pub build: Option<String>,
    /// Product name
    #[serde(default)]
    pub product_name: Option<String>,
    /// Product version (e.g. "2024.1.0.12")
    #[serde(default)]
    pub product_version: Option<String>,
    /// N-central version
    #[serde(default)]
    pub ncentral_version: Option<String>,
    /// N-central version (alternate field name)
    #[serde(default)]
    pub ncentral: Option<String>,
    /// Capture any other fields
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}
