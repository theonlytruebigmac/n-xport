//! N-Central SOAP API Client
//!
//! This module provides SOAP API support for N-Central operations
//! that are not available via REST API (e.g., userAdd).

use reqwest::Client;

/// SOAP API endpoint path
const SOAP_ENDPOINT: &str = "/dms2/services2/ServerEI2";

/// Error type for SOAP operations
#[derive(Debug)]
pub enum SoapError {
    /// HTTP request failed
    HttpError(String),
    /// SOAP fault returned
    SoapFault { code: String, message: String },
    /// Failed to parse response
    ParseError(String),
    /// Authentication error
    AuthError(String),
}

impl std::fmt::Display for SoapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoapError::HttpError(e) => write!(f, "HTTP error: {}", e),
            SoapError::SoapFault { code, message } => {
                write!(f, "SOAP fault [{}]: {}", code, message)
            }
            SoapError::ParseError(e) => write!(f, "Parse error: {}", e),
            SoapError::AuthError(e) => write!(f, "Auth error: {}", e),
        }
    }
}

impl std::error::Error for SoapError {}

/// User information for userAdd operation
#[derive(Debug, Clone)]
pub struct UserAddInfo {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub location: Option<String>,
    pub is_enabled: bool,
    /// Customer ID where user should be created
    pub customer_id: i64,
    /// Role IDs to assign to the user
    pub role_ids: Vec<i64>,
    /// Access group IDs to assign to the user
    pub access_group_ids: Vec<i64>,
}

/// N-Central SOAP Client
pub struct NcSoapClient {
    http_client: Client,
    base_url: String,
    jwt: String,
    username: Option<String>,
}

impl NcSoapClient {
    /// Create a new SOAP client
    pub fn new(base_url: &str, jwt: &str) -> Self {
        Self {
            http_client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            jwt: jwt.to_string(),
            username: None,
        }
    }

    /// Set API username for SOAP operations (password is the JWT)
    pub fn set_username(&mut self, username: &str) {
        self.username = Some(username.to_string());
    }

    /// Build the SOAP endpoint URL
    fn endpoint_url(&self) -> String {
        format!("{}{}", self.base_url, SOAP_ENDPOINT)
    }

    /// Build SOAP envelope for userAdd
    fn build_user_add_envelope(&self, username: &str, info: &UserAddInfo) -> String {
        let mut settings = Vec::new();

        // Generate strong password meeting requirements first (mandatory)
        let new_user_password = generate_strong_password();

        // Mandatory fields (per N-Central SOAP API docs)
        settings.push(("email", info.email.clone()));
        settings.push(("password", new_user_password));
        settings.push(("customerID", info.customer_id.to_string()));
        settings.push(("firstname", info.first_name.clone()));
        settings.push(("lastname", info.last_name.clone()));

        // Optional but useful fields
        settings.push(("username", username.to_string()));
        settings.push((
            "status",
            if info.is_enabled {
                "enabled".to_string()
            } else {
                "disabled".to_string()
            },
        ));

        // Optional fields
        if let Some(ref phone) = info.phone {
            settings.push(("phone", phone.clone()));
        }
        if let Some(ref dept) = info.department {
            settings.push(("department", dept.clone()));
        }
        if let Some(ref loc) = info.location {
            settings.push(("location", loc.clone()));
        }

        // Role IDs - key is "userroleID" per N-Central API
        if !info.role_ids.is_empty() {
            let roles_str = info
                .role_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            tracing::info!("SOAP userAdd: setting userroleID = '{}'", roles_str);
            settings.push(("userroleID", roles_str));
        } else {
            tracing::warn!("SOAP userAdd: no role_ids provided for user");
        }

        // Access group IDs
        if !info.access_group_ids.is_empty() {
            let groups_str = info
                .access_group_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            settings.push(("accessgroupids", groups_str));
        }

        // Enforce password change on next login
        settings.push(("mustchangepassword", "true".to_string()));

        // Build settings XML (repeated 'settings' elements)
        let settings_xml: String = settings
            .iter()
            .map(|(key, value)| {
                format!(
                    r#"<ei2:settings>
                <ei2:key>{}</ei2:key>
                <ei2:value>{}</ei2:value>
            </ei2:settings>"#,
                    xml_escape(key),
                    xml_escape(value)
                )
            })
            .collect::<Vec<_>>()
            .join("\n         ");

        // Determine credentials to use for API Authentication
        // SOAP requires: username = API username, password = JWT token
        let (api_user, api_pass) = if let Some(u) = &self.username {
            // Use username + JWT as password (SOAP auth pattern)
            (u.as_str(), self.jwt.as_str())
        } else {
            // No username provided - fallback (likely fails for userAdd)
            tracing::warn!("No API username provided for SOAP authentication - userAdd may fail");
            ("", self.jwt.as_str())
        };

        // Build full SOAP envelope
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" 
                  xmlns:ei2="http://ei2.nobj.nable.com/">
   <soapenv:Header/>
   <soapenv:Body>
      <ei2:userAdd>
         <ei2:username>{}</ei2:username>
         <ei2:password>{}</ei2:password>
         {}
      </ei2:userAdd>
   </soapenv:Body>
</soapenv:Envelope>"#,
            xml_escape(api_user),
            xml_escape(api_pass),
            settings_xml
        )
    }

    /// Add a new user via SOAP API
    ///
    /// Returns the new user's ID on success
    pub async fn user_add(&self, username: &str, info: &UserAddInfo) -> Result<i64, SoapError> {
        let envelope = self.build_user_add_envelope(username, info);

        tracing::info!(
            "SOAP userAdd request to {} (base_url: {})",
            self.endpoint_url(),
            self.base_url
        );
        tracing::info!(
            "userAdd data: username='{}', email='{}', firstName='{}', lastName='{}', customerID={}",
            username,
            info.email,
            info.first_name,
            info.last_name,
            info.customer_id
        );
        // Do not log full envelope to avoid leaking generated password
        // tracing::trace!("SOAP envelope: {}", envelope);

        let mut request = self
            .http_client
            .post(&self.endpoint_url())
            .header("Content-Type", "text/xml; charset=utf-8")
            .header("SOAPAction", "\"\""); // Empty action matching JAX-WS standard

        // Use Authorization header only if we don't have explicit credentials (as fallback)
        if self.username.is_none() {
            request = request.header("Authorization", format!("Bearer {}", self.jwt));
        }

        let response = request
            .body(envelope)
            .send()
            .await
            .map_err(|e| SoapError::HttpError(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| SoapError::ParseError(e.to_string()))?;

        tracing::debug!("SOAP response status: {}", status);
        // Temporarily log full response for debugging
        tracing::info!(
            "SOAP userAdd response body: {}",
            &body[..body.len().min(500)]
        );

        if !status.is_success() {
            // Check for SOAP fault
            if let Some(fault) = parse_soap_fault(&body) {
                return Err(fault);
            }
            return Err(SoapError::HttpError(format!(
                "HTTP {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            )));
        }

        // Parse success response for user ID
        let user_id = parse_user_add_response(&body)?;
        if user_id == -1 {
            tracing::warn!("userAdd returned ID -1 - this might indicate user already exists or another issue. Full response: {}", &body[..body.len().min(1000)]);
        }
        Ok(user_id)
    }
}

/// Generate a strong password meeting N-Central requirements
/// Requirements: At least 8 characters, 1 number, 1 uppercase, 1 lowercase, 1 special
fn generate_strong_password() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Character sets
    let upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let lower = "abcdefghijklmnopqrstuvwxyz";
    let numbers = "0123456789";
    let special = "!@#$%^&*()_+-=[]{}|;:,.<>?";

    // Simple pseudo-random generator using time
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    // Linear Congruential Generator parameters (from Numerical Recipes)
    let a: u128 = 1664525;
    let c: u128 = 1013904223;
    let m: u128 = 2u128.pow(32);

    let mut next_rand = || {
        seed = (a.wrapping_mul(seed).wrapping_add(c)) % m;
        seed as usize
    };

    let mut password = String::new();

    // Ensure one of each required type
    password.push(upper.as_bytes()[next_rand() % upper.len()] as char);
    password.push(lower.as_bytes()[next_rand() % lower.len()] as char);
    password.push(numbers.as_bytes()[next_rand() % numbers.len()] as char);
    password.push(special.as_bytes()[next_rand() % special.len()] as char);

    // Fill remaining 8 chars (total 12)
    let all_chars = format!("{}{}{}{}", upper, lower, numbers, special);
    for _ in 0..8 {
        password.push(all_chars.as_bytes()[next_rand() % all_chars.len()] as char);
    }

    // Shuffle the password loosely (swap random positions)
    let mut pwd_chars: Vec<char> = password.chars().collect();
    for i in 0..pwd_chars.len() {
        let j = next_rand() % pwd_chars.len();
        pwd_chars.swap(i, j);
    }

    pwd_chars.into_iter().collect()
}

/// Escape special XML characters
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Parse SOAP fault from response
fn parse_soap_fault(body: &str) -> Option<SoapError> {
    // Simple fault detection
    if body.contains("<faultcode>") || body.contains("<soap:Fault>") || body.contains("<Fault>") {
        // Extract fault code
        let code = extract_xml_value(body, "faultcode").unwrap_or_else(|| "Unknown".to_string());
        // Extract fault message
        let message =
            extract_xml_value(body, "faultstring").unwrap_or_else(|| "Unknown error".to_string());

        return Some(SoapError::SoapFault { code, message });
    }
    None
}

/// Parse userAdd response to get new user ID
fn parse_user_add_response(body: &str) -> Result<i64, SoapError> {
    // Look for return value in response
    // The response format is typically: <return>USER_ID</return>
    if let Some(id_str) = extract_xml_value(body, "return") {
        id_str
            .parse::<i64>()
            .map_err(|e| SoapError::ParseError(format!("Failed to parse user ID: {}", e)))
    } else {
        // Try alternative tag names
        if let Some(id_str) = extract_xml_value(body, "userAddReturn") {
            id_str
                .parse::<i64>()
                .map_err(|e| SoapError::ParseError(format!("Failed to parse user ID: {}", e)))
        } else {
            Err(SoapError::ParseError(
                "Could not find user ID in response".to_string(),
            ))
        }
    }
}

/// Simple XML value extraction (avoids full XML parser dependency)
fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    // Try with namespace prefix
    for prefix in &["", "ns1:", "ei2:", "soap:"] {
        let open_tag = format!("<{}{}>", prefix, tag);
        let close_tag = format!("</{}{}>", prefix, tag);

        if let Some(start) = xml.find(&open_tag) {
            let value_start = start + open_tag.len();
            if let Some(end) = xml[value_start..].find(&close_tag) {
                return Some(xml[value_start..value_start + end].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xml_escape() {
        assert_eq!(xml_escape("a<b>c"), "a&lt;b&gt;c");
        assert_eq!(xml_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn test_extract_xml_value() {
        let xml = "<response><return>12345</return></response>";
        assert_eq!(extract_xml_value(xml, "return"), Some("12345".to_string()));
    }
}
