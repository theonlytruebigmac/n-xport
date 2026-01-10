//! Connection-related Tauri commands

use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::api::NcClient;
use crate::api::NcSoapClient;
use crate::credentials::CredentialStore;

/// Shared client state
pub struct AppState {
    pub client: Arc<Mutex<Option<NcClient>>>,
    pub dest_client: Arc<Mutex<Option<NcClient>>>,
    /// SOAP client for destination (for operations not available via REST)
    pub dest_soap_client: Arc<Mutex<Option<NcSoapClient>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            dest_client: Arc::new(Mutex::new(None)),
            dest_soap_client: Arc::new(Mutex::new(None)),
        }
    }
}

/// Connection test result
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResult {
    pub success: bool,
    pub message: String,
    pub server_url: Option<String>,
    pub server_version: Option<String>,
    pub service_org_id: Option<i64>,
    pub service_org_name: Option<String>,
}

/// Test connection to N-Central server
#[tauri::command]
pub async fn test_connection(
    fqdn: String,
    jwt: String,
    _username: Option<String>,
    _password: Option<String>,
    state: State<'_, AppState>,
) -> std::result::Result<ConnectionResult, String> {
    let jwt = jwt.trim().to_string();
    // ... existing logic (username/password not used for simple REST test yet)
    let base_url = format!(
        "https://{}",
        fqdn.trim_start_matches("https://")
            .trim_start_matches("http://")
    );

    let client = NcClient::new(&base_url);

    // Authenticate
    if let Err(e) = client.authenticate(&jwt).await {
        return Ok(ConnectionResult {
            success: false,
            message: format!("Authentication failed: {}", e),
            server_url: Some(base_url),
            server_version: None,
            service_org_id: None,
            service_org_name: None,
        });
    }

    // Get server info
    let version = match client.get_server_info().await {
        Ok(info) => {
            tracing::info!("Server info: {:?}", info);
            // Prefer fields that likely contain the full version string
            info.ncentral
                .or(info.product_version)
                .or(info.ncentral_version)
                .or(info.version)
                .or(info.build)
                .or(info.api_version)
        }
        Err(e) => {
            tracing::warn!("Could not get server version: {}", e);
            None
        }
    };

    // Get first service org info
    let (so_id, so_name) = match client.get_service_orgs().await {
        Ok(orgs) if !orgs.is_empty() => (Some(orgs[0].so_id), Some(orgs[0].so_name.clone())),
        _ => (None, None),
    };

    // Store client for later use
    *state.client.lock().await = Some(client);

    Ok(ConnectionResult {
        success: true,
        message: "Connection successful".to_string(),
        server_url: Some(base_url),
        server_version: version,
        service_org_id: so_id,
        service_org_name: so_name,
    })
}

/// Connect using saved credentials
#[tauri::command]
pub async fn connect_with_profile(
    profile_name: String,
    fqdn: String,
    username: Option<String>,
    state: State<'_, AppState>,
) -> std::result::Result<ConnectionResult, String> {
    // Get JWT from keychain
    let jwt = match CredentialStore::get_jwt(&profile_name) {
        Ok(Some(jwt)) => jwt,
        Ok(None) => {
            return Ok(ConnectionResult {
                success: false,
                message: "No saved credentials for this profile".to_string(),
                server_url: None,
                server_version: None,
                service_org_id: None,
                service_org_name: None,
            });
        }
        Err(e) => {
            return Ok(ConnectionResult {
                success: false,
                message: format!("Failed to retrieve credentials: {}", e),
                server_url: None,
                server_version: None,
                service_org_id: None,
                service_org_name: None,
            });
        }
    };

    // Get Password from keychain (optional, ignore errors effectively by treating as None)
    let password = CredentialStore::get_password(&profile_name).unwrap_or(None);

    test_connection(fqdn, jwt, username, password, state).await
}

/// Test connection specifically for destination server
#[tauri::command]
pub async fn connect_destination(
    fqdn: String,
    jwt: String,
    username: Option<String>,
    _password: Option<String>,
    state: State<'_, AppState>,
) -> std::result::Result<ConnectionResult, String> {
    let jwt = jwt.trim().to_string();
    let base_url = format!(
        "https://{}",
        fqdn.trim_start_matches("https://")
            .trim_start_matches("http://")
    );

    let client = NcClient::new(&base_url);

    // Authenticate
    if let Err(e) = client.authenticate(&jwt).await {
        return Ok(ConnectionResult {
            success: false,
            message: format!("Authentication failed: {}", e),
            server_url: Some(base_url),
            server_version: None,
            service_org_id: None,
            service_org_name: None,
        });
    }

    // Get server info
    let version = match client.get_server_info().await {
        Ok(info) => info
            .ncentral
            .or(info.product_version)
            .or(info.ncentral_version)
            .or(info.version)
            .or(info.build)
            .or(info.api_version),
        Err(_) => None,
    };

    // Get first service org info
    let (so_id, so_name) = match client.get_service_orgs().await {
        Ok(orgs) if !orgs.is_empty() => (Some(orgs[0].so_id), Some(orgs[0].so_name.clone())),
        _ => (None, None),
    };

    // Store destination client
    *state.dest_client.lock().await = Some(client);

    // Initialize & store SOAP client for destination
    // Pass username if available - JWT is used as password
    let mut soap_client = NcSoapClient::new(&base_url, &jwt);
    if let Some(u) = username {
        soap_client.set_username(&u);
    }

    *state.dest_soap_client.lock().await = Some(soap_client);

    Ok(ConnectionResult {
        success: true,
        message: "Destination connection successful".to_string(),
        server_url: Some(base_url),
        server_version: version,
        service_org_id: so_id,
        service_org_name: so_name,
    })
}

/// Save credentials (JWT and optional Password) for a profile
#[tauri::command]
pub async fn save_credentials(
    profile_name: String,
    jwt: String,
    password: Option<String>,
) -> std::result::Result<(), String> {
    let jwt = jwt.trim().to_string();

    // Store JWT
    if let Err(e) = CredentialStore::store_jwt(&profile_name, &jwt) {
        tracing::error!(
            "Failed to save JWT to keyring for '{}': {}",
            profile_name,
            e
        );
        return Err(format!(
            "Failed to save credentials: {}. Please ensure your system keyring is available.",
            e
        ));
    }

    // Store Password if provided
    if let Some(pwd) = password {
        if let Err(e) = CredentialStore::store_password(&profile_name, &pwd) {
            tracing::error!(
                "Failed to save password to keyring for '{}': {}",
                profile_name,
                e
            );
            // Don't fail the whole operation, but log error
        }
    }

    // Verification logic
    tracing::info!(
        "Successfully saved credentials to keyring for '{}'",
        profile_name
    );

    // Optional verification - warn but don't fail if it doesn't work
    // Linux Secret Service can have timing issues
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    match CredentialStore::get_jwt(&profile_name) {
        Ok(Some(stored_jwt)) if stored_jwt == jwt => {
            tracing::debug!("Verification successful for '{}'", profile_name);
        }
        _ => {
            tracing::warn!(
                "Could not verify credential persistence for '{}'. This may be a keyring timing issue - credentials were likely saved successfully.",
                profile_name
            );
        }
    }
    Ok(())
}

/// Check if credentials exist for a profile
#[tauri::command]
pub async fn has_credentials(profile_name: String) -> bool {
    // Check keyring first
    match CredentialStore::get_jwt(&profile_name) {
        Ok(Some(_)) => return true,
        _ => {}
    }

    // No fallback - keyring only
    false
}

/// Get credentials for a profile
#[tauri::command]
pub async fn get_credentials(profile_name: String) -> std::result::Result<Option<String>, String> {
    tracing::info!("Getting credentials for '{}'", profile_name);

    // Get credentials from OS keyring only (no fallback for security)
    match CredentialStore::get_jwt(&profile_name) {
        Ok(Some(jwt)) => {
            tracing::info!("Found credentials in keyring for '{}'", profile_name);
            Ok(Some(jwt))
        }
        Ok(None) => {
            tracing::warn!("No credentials in keyring for '{}'", profile_name);
            Ok(None)
        }
        Err(e) => {
            tracing::error!("Keyring error for '{}': {}", profile_name, e);
            Ok(None)
        }
    }
}

/// Delete credentials for a profile
#[tauri::command]
pub async fn delete_credentials(profile_name: String) -> std::result::Result<(), String> {
    tracing::info!("Deleting credentials for '{}'", profile_name);

    // Delete from keyring only (no fallback)
    let _ = CredentialStore::delete_jwt(&profile_name);
    let _ = CredentialStore::delete_password(&profile_name);

    // Also delete destination credentials if they exist
    let dest_profile = format!("{}_dest", profile_name);
    let _ = CredentialStore::delete_jwt(&dest_profile);
    let _ = CredentialStore::delete_password(&dest_profile);

    Ok(())
}

/// Get password for a profile
#[tauri::command]
pub async fn get_password(profile_name: String) -> std::result::Result<Option<String>, String> {
    // Get password from OS keyring
    match CredentialStore::get_password(&profile_name) {
        Ok(pwd) => Ok(pwd),
        Err(e) => {
            tracing::warn!("Failed to retrieve password for '{}': {}", profile_name, e);
            Ok(None)
        }
    }
}

/// Disconnect (clear client)
#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> std::result::Result<(), String> {
    *state.client.lock().await = None;
    *state.dest_client.lock().await = None;
    Ok(())
}

/// Get info about a specific service organization by ID
#[tauri::command]
pub async fn get_service_org_info(
    service_org_id: i64,
    state: State<'_, AppState>,
) -> std::result::Result<serde_json::Value, String> {
    let client = state.client.lock().await;

    let client = match &*client {
        Some(c) => c,
        None => return Err("Not connected".to_string()),
    };

    match client.get_service_org_by_id(service_org_id).await {
        Ok(so) => Ok(serde_json::json!({
            "id": so.so_id,
            "name": so.so_name
        })),
        Err(e) => Err(format!("Failed to get service org: {}", e)),
    }
}
