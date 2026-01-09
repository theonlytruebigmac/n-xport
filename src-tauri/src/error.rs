//! Error types for the N-Central Data Export application

use thiserror::Error;

/// Application-level errors
#[derive(Error, Debug)]
pub enum AppError {
    #[error("API error: {0}")]
    Api(#[from] ApiError),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Export error: {0}")]
    Export(String),

    #[error("Credential error: {0}")]
    Credential(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// API-specific errors
#[derive(Error, Debug)]
pub enum ApiError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Rate limited - retry after {retry_after_secs} seconds")]
    RateLimited { retry_after_secs: u64 },

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Server error: {status} - {message}")]
    Server { status: u16, message: String },

    #[error("Token expired")]
    TokenExpired,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

/// Result type alias for AppError
pub type Result<T> = std::result::Result<T, AppError>;

/// Result type alias for ApiError
pub type ApiResult<T> = std::result::Result<T, ApiError>;

// Implement conversion for Tauri commands
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
