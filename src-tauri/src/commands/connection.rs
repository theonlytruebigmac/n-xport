//! Connection-related Tauri commands

use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::api::NcClient;
use crate::config::Settings;
use crate::credentials::CredentialStore;

/// Shared client state
pub struct AppState {
    pub client: Arc<Mutex<Option<NcClient>>>,
    pub dest_client: Arc<Mutex<Option<NcClient>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            dest_client: Arc::new(Mutex::new(None)),
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

    test_connection(fqdn, jwt, state).await
}

/// Test connection specifically for destination server
#[tauri::command]
pub async fn connect_destination(
    fqdn: String,
    jwt: String,
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

    Ok(ConnectionResult {
        success: true,
        message: "Destination connection successful".to_string(),
        server_url: Some(base_url),
        server_version: version,
        service_org_id: so_id,
        service_org_name: so_name,
    })
}

/// Save credentials (JWT) for a profile
#[tauri::command]
pub async fn save_credentials(
    profile_name: String,
    jwt: String,
) -> std::result::Result<(), String> {
    let jwt = jwt.trim().to_string();

    // Store credentials in OS keyring only (no fallback for security)
    match CredentialStore::store_jwt(&profile_name, &jwt) {
        Ok(_) => {
            tracing::info!(
                "Successfully saved credentials to keyring for '{}'",
                profile_name
            );
            Ok(())
        }
        Err(e) => {
            tracing::error!(
                "Failed to save credentials to keyring for '{}': {}",
                profile_name,
                e
            );
            Err(format!(
                "Failed to save credentials: {}. Please ensure your system keyring is available.",
                e
            ))
        }
    }
}

/// Check if credentials exist for a profile
#[tauri::command]
pub async fn has_credentials(profile_name: String) -> bool {
    // Check keyring first
    match CredentialStore::get_jwt(&profile_name) {
        Ok(Some(_)) => return true,
        _ => {}
    }

    // Check fallback
    if let Ok(settings) = Settings::load() {
        if let Some(profile) = settings.profiles.iter().find(|p| p.name == profile_name) {
            if profile.encrypted_jwt.is_some() {
                return true;
            }
        }
    }

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

    // Delete from keyring
    let _ = CredentialStore::delete_jwt(&profile_name);

    // Delete fallback
    if let Ok(mut settings) = Settings::load() {
        if let Some(profile) = settings
            .profiles
            .iter_mut()
            .find(|p| p.name == profile_name)
        {
            profile.encrypted_jwt = None;
            let _ = settings.save();
        }
    }

    Ok(())
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
