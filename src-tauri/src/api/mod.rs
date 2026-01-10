//! API client module

pub mod auth;
pub mod client;
pub mod endpoints;
pub mod rate_limiter;
pub mod soap_client;

pub use auth::AuthManager;
pub use client::NcClient;
pub use rate_limiter::RateLimiter;
pub use soap_client::{NcSoapClient, SoapError, UserAddInfo};
