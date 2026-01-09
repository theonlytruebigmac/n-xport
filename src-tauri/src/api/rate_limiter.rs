//! Concurrency-based rate limiter for N-Central API
//!
//! Uses semaphores to limit concurrent requests per endpoint type,
//! matching N-Central's official rate limits.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Rate limit configuration per endpoint
#[derive(Debug, Clone)]
pub struct EndpointLimits {
    /// Default concurrent limit for unlisted endpoints
    pub default: u32,
    /// Per-endpoint limits
    pub endpoints: HashMap<String, u32>,
}

impl Default for EndpointLimits {
    fn default() -> Self {
        let mut endpoints = HashMap::new();
        
        // Auth endpoints - high concurrency allowed
        endpoints.insert("/api/auth/authenticate".into(), 50);
        endpoints.insert("/api/auth/refresh".into(), 50);
        endpoints.insert("/api/auth/validate".into(), 50);
        endpoints.insert("/api/health".into(), 50);
        endpoints.insert("/api/server-info".into(), 50);
        
        // Data listing endpoints - limited to 5 concurrent
        endpoints.insert("/api/service-orgs".into(), 5);
        endpoints.insert("/api/customers".into(), 5);
        endpoints.insert("/api/sites".into(), 5);
        endpoints.insert("/api/devices".into(), 5);
        endpoints.insert("/api/org-units".into(), 5);
        endpoints.insert("/api/users".into(), 5);
        endpoints.insert("/api/device-filters".into(), 5);
        
        // Single resource endpoints - higher limit
        endpoints.insert("/api/devices/{id}".into(), 50);
        endpoints.insert("/api/devices/{id}/assets".into(), 50);
        
        // Property endpoints - limited
        endpoints.insert("/api/devices/{id}/custom-properties".into(), 5);
        endpoints.insert("/api/org-units/{id}/custom-properties".into(), 5);
        endpoints.insert("/api/org-units/{id}/access-groups".into(), 5);
        endpoints.insert("/api/org-units/{id}/user-roles".into(), 5);
        endpoints.insert("/api/org-units/{id}/devices".into(), 5);
        endpoints.insert("/api/org-units/{id}/active-issues".into(), 3);
        
        Self {
            default: 5,
            endpoints,
        }
    }
}

/// Rate limiter using semaphores for concurrency control
pub struct RateLimiter {
    /// Semaphores per endpoint pattern
    semaphores: HashMap<String, Arc<Semaphore>>,
    /// Default semaphore for unlisted endpoints
    default_semaphore: Arc<Semaphore>,
    /// Configuration
    limits: EndpointLimits,
}

impl RateLimiter {
    /// Create a new rate limiter with default limits
    pub fn new() -> Self {
        Self::with_limits(EndpointLimits::default())
    }

    /// Create a rate limiter with custom limits
    pub fn with_limits(limits: EndpointLimits) -> Self {
        let semaphores = limits
            .endpoints
            .iter()
            .map(|(endpoint, limit)| {
                (endpoint.clone(), Arc::new(Semaphore::new(*limit as usize)))
            })
            .collect();

        Self {
            default_semaphore: Arc::new(Semaphore::new(limits.default as usize)),
            semaphores,
            limits,
        }
    }

    /// Get the semaphore for a given endpoint path
    fn get_semaphore(&self, path: &str) -> Arc<Semaphore> {
        // Try exact match first
        if let Some(sem) = self.semaphores.get(path) {
            return sem.clone();
        }

        // Try pattern matching for parameterized endpoints
        let normalized = self.normalize_path(path);
        if let Some(sem) = self.semaphores.get(&normalized) {
            return sem.clone();
        }

        // Fall back to default
        self.default_semaphore.clone()
    }

    /// Normalize a path by replacing IDs with {id} placeholder
    fn normalize_path(&self, path: &str) -> String {
        let parts: Vec<&str> = path.split('/').collect();
        let normalized: Vec<String> = parts
            .iter()
            .map(|part| {
                // Replace numeric IDs with {id}
                if part.parse::<i64>().is_ok() {
                    "{id}".to_string()
                } else {
                    (*part).to_string()
                }
            })
            .collect();
        normalized.join("/")
    }

    /// Acquire a permit for the given endpoint
    /// Returns a guard that releases the permit when dropped
    pub async fn acquire(&self, path: &str) -> RateLimitGuard {
        let semaphore = self.get_semaphore(path);
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("Semaphore closed unexpectedly");
        
        RateLimitGuard { _permit: permit }
    }

    /// Get the limit for a given endpoint
    pub fn get_limit(&self, path: &str) -> u32 {
        let normalized = self.normalize_path(path);
        self.limits
            .endpoints
            .get(&normalized)
            .or_else(|| self.limits.endpoints.get(path))
            .copied()
            .unwrap_or(self.limits.default)
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard that releases the rate limit permit when dropped
pub struct RateLimitGuard {
    _permit: tokio::sync::OwnedSemaphorePermit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        let limiter = RateLimiter::new();
        
        assert_eq!(
            limiter.normalize_path("/api/devices/12345"),
            "/api/devices/{id}"
        );
        assert_eq!(
            limiter.normalize_path("/api/devices/12345/custom-properties"),
            "/api/devices/{id}/custom-properties"
        );
        assert_eq!(
            limiter.normalize_path("/api/service-orgs"),
            "/api/service-orgs"
        );
    }

    #[test]
    fn test_get_limit() {
        let limiter = RateLimiter::new();
        
        assert_eq!(limiter.get_limit("/api/auth/authenticate"), 50);
        assert_eq!(limiter.get_limit("/api/devices"), 5);
        assert_eq!(limiter.get_limit("/api/devices/12345"), 50);
        assert_eq!(limiter.get_limit("/api/unknown"), 5); // default
    }
}
