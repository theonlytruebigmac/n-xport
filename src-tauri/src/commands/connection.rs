//! Connection-related Tauri commands

use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::api::NcClient;
use crate::api::NcSoapClient;
use crate::credentials::{CredentialStore, StorageBackend};
use crate::import::handlers::ImportContext;

/// Cached name-lookup tables from a previous import. Keyed by the SO they were
/// built for so a dry-run → live-run transition (same SO, seconds apart) can
/// reuse the same context instead of re-fetching every customer/site/role.
pub struct CachedImportContext {
    pub service_org_id: i64,
    pub ctx: ImportContext,
}

/// Shared client state
pub struct AppState {
    pub client: Arc<Mutex<Option<NcClient>>>,
    /// SOAP client for source (for operations not available via REST, e.g. user_add during import)
    pub source_soap_client: Arc<Mutex<Option<NcSoapClient>>>,
    pub dest_client: Arc<Mutex<Option<NcClient>>>,
    /// SOAP client for destination (for operations not available via REST)
    pub dest_soap_client: Arc<Mutex<Option<NcSoapClient>>>,
    /// Cancellation token for long-running operations
    pub cancel_token: Arc<AtomicBool>,
    /// Cached ImportContext from the most recent import. Survives across
    /// `start_import` calls so a follow-up live run after a successful dry-run
    /// skips the lookup-build phase. Cleared on disconnect.
    pub import_context_cache: Arc<Mutex<Option<CachedImportContext>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            source_soap_client: Arc::new(Mutex::new(None)),
            dest_client: Arc::new(Mutex::new(None)),
            dest_soap_client: Arc::new(Mutex::new(None)),
            cancel_token: Arc::new(AtomicBool::new(false)),
            import_context_cache: Arc::new(Mutex::new(None)),
        }
    }
}

impl AppState {
    /// Drop everything tied to the source connection: the REST and SOAP clients and
    /// the import lookup cache (which holds IDs from that server, so it must never
    /// outlive the connection it was built from).
    pub async fn clear_source(&self) {
        *self.client.lock().await = None;
        *self.source_soap_client.lock().await = None;
        *self.import_context_cache.lock().await = None;
    }

    /// Drop the destination REST and SOAP clients.
    pub async fn clear_destination(&self) {
        *self.dest_client.lock().await = None;
        *self.dest_soap_client.lock().await = None;
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

impl ConnectionResult {
    /// Create a failure result with just a message
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            server_url: None,
            server_version: None,
            service_org_id: None,
            service_org_name: None,
        }
    }
}

/// The https URL of a server from whatever was typed or pasted: with or without a scheme,
/// surrounded by spaces, or followed by a slash or a page path.
pub(crate) fn server_url(input: &str) -> String {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();
    let without_scheme = if lower.starts_with("https://") {
        &trimmed[8..]
    } else if lower.starts_with("http://") {
        &trimmed[7..]
    } else {
        trimmed
    };
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    format!("https://{}", host)
}

/// Shared connection logic: normalize URL, authenticate, fetch server info & service org.
/// Returns the connected client and a successful ConnectionResult on success.
async fn establish_connection(
    fqdn: &str,
    jwt: &str,
) -> std::result::Result<(NcClient, ConnectionResult), ConnectionResult> {
    let jwt = jwt.trim();
    let base_url = server_url(fqdn);

    let client = NcClient::new(&base_url);

    // Authenticate
    if let Err(e) = client.authenticate(jwt).await {
        return Err(ConnectionResult {
            success: false,
            message: format!("Authentication failed: {}", e),
            server_url: Some(base_url),
            ..ConnectionResult::failure("")
        });
    }

    // Get server info
    let version = match client.get_server_info().await {
        Ok(info) => {
            tracing::info!("Server info: {:?}", info);
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

    let result = ConnectionResult {
        success: true,
        message: "Connection successful".to_string(),
        server_url: Some(base_url),
        server_version: version,
        service_org_id: so_id,
        service_org_name: so_name,
    };

    Ok((client, result))
}

/// Test connection to N-Central server
#[tauri::command]
pub async fn test_connection(
    fqdn: String,
    jwt: String,
    username: Option<String>,
    state: State<'_, AppState>,
) -> std::result::Result<ConnectionResult, String> {
    let base_url = server_url(&fqdn);

    // Connecting replaces the source connection. Clear the previous one first so a
    // failed attempt cannot leave the old server connected behind a "disconnected" UI,
    // and so lookups cached from the old server are never reused for the new one.
    state.clear_source().await;

    match establish_connection(&fqdn, &jwt).await {
        Ok((client, result)) => {
            *state.client.lock().await = Some(client);

            // Initialize & store SOAP client for source (used by import for user_add etc.)
            let mut soap_client = NcSoapClient::new(&base_url, jwt.trim());
            if let Some(u) = username {
                soap_client.set_username(&u);
            }
            *state.source_soap_client.lock().await = Some(soap_client);

            Ok(result)
        }
        Err(result) => Ok(result),
    }
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
            return Ok(ConnectionResult::failure(
                "No saved credentials for this profile",
            ))
        }
        Err(e) => {
            return Ok(ConnectionResult::failure(format!(
                "Failed to retrieve credentials: {}",
                e
            )))
        }
    };

    test_connection(fqdn, jwt, username, state).await
}

/// Test connection specifically for destination server
#[tauri::command]
pub async fn connect_destination(
    fqdn: String,
    jwt: String,
    username: Option<String>,
    state: State<'_, AppState>,
) -> std::result::Result<ConnectionResult, String> {
    let base_url = server_url(&fqdn);

    // Same rule as the source: a failed attempt must not leave the old destination live.
    state.clear_destination().await;

    match establish_connection(&fqdn, &jwt).await {
        Ok((client, mut result)) => {
            result.message = "Destination connection successful".to_string();

            // Store destination REST client
            *state.dest_client.lock().await = Some(client);

            // Initialize & store SOAP client for destination
            let mut soap_client = NcSoapClient::new(&base_url, jwt.trim());
            if let Some(u) = username {
                soap_client.set_username(&u);
            }
            *state.dest_soap_client.lock().await = Some(soap_client);

            Ok(result)
        }
        Err(result) => Ok(result),
    }
}

/// Result of saving credentials: where they ended up, and what the user should be told.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCredentialsResult {
    /// Where the JWT was stored.
    pub storage: StorageBackend,
    /// Present when something needs the user's attention (insecure fallback storage,
    /// or an optional password that could not be saved).
    pub warning: Option<String>,
}

/// Save credentials (JWT and optional Password) for a profile
#[tauri::command]
pub async fn save_credentials(
    profile_name: String,
    jwt: String,
    password: Option<String>,
) -> std::result::Result<SaveCredentialsResult, String> {
    let jwt = jwt.trim().to_string();

    let storage = CredentialStore::store_jwt(&profile_name, &jwt).map_err(|e| {
        tracing::error!("Failed to save JWT for '{}': {}", profile_name, e);
        format!("Failed to save credentials: {}", e)
    })?;
    let mut warnings: Vec<String> = storage.warning().map(str::to_string).into_iter().collect();

    // The password is optional: a failure does not fail the save, but the user is told.
    if let Some(pwd) = password {
        match CredentialStore::store_password(&profile_name, &pwd) {
            Ok(password_storage) => {
                if storage == StorageBackend::Keychain {
                    warnings.extend(password_storage.warning().map(str::to_string));
                }
            }
            Err(e) => {
                tracing::error!("Failed to save password for '{}': {}", profile_name, e);
                warnings.push(format!("The password could not be saved: {}", e));
            }
        }
    }

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

    Ok(SaveCredentialsResult {
        storage,
        warning: (!warnings.is_empty()).then(|| warnings.join(" ")),
    })
}

/// Check if credentials exist for a profile (keychain, then the fallback file)
#[tauri::command]
pub async fn has_credentials(profile_name: String) -> bool {
    CredentialStore::has_jwt(&profile_name)
}

/// Get credentials for a profile
#[tauri::command]
pub async fn get_credentials(profile_name: String) -> std::result::Result<Option<String>, String> {
    tracing::info!("Getting credentials for '{}'", profile_name);

    // Keychain first, then the fallback file
    match CredentialStore::get_jwt(&profile_name) {
        Ok(Some(jwt)) => {
            tracing::info!("Found stored credentials for '{}'", profile_name);
            Ok(Some(jwt))
        }
        Ok(None) => {
            tracing::warn!("No stored credentials for '{}'", profile_name);
            Ok(None)
        }
        Err(e) => {
            tracing::error!("Credential lookup failed for '{}': {}", profile_name, e);
            Ok(None)
        }
    }
}

/// Delete credentials for a profile, including its destination credentials, from
/// both the keychain and the fallback file. Fails if any copy could not be removed.
#[tauri::command]
pub async fn delete_credentials(profile_name: String) -> std::result::Result<(), String> {
    tracing::info!("Deleting credentials for '{}'", profile_name);
    CredentialStore::delete_profile_credentials(&profile_name).map_err(|e| {
        tracing::error!("Failed to delete credentials for '{}': {}", profile_name, e);
        format!("Some stored credentials could not be removed: {}", e)
    })
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
    state.clear_source().await;
    state.clear_destination().await;
    Ok(())
}

/// Get info about a specific service organization by ID
#[tauri::command]
pub async fn get_service_org_info(
    service_org_id: i64,
    destination: Option<bool>,
    state: State<'_, AppState>,
) -> std::result::Result<serde_json::Value, String> {
    get_service_org_info_for(&state, service_org_id, destination.unwrap_or(false)).await
}

async fn get_service_org_info_for(
    state: &AppState,
    service_org_id: i64,
    destination: bool,
) -> std::result::Result<serde_json::Value, String> {
    let slot = if destination {
        &state.dest_client
    } else {
        &state.client
    };
    let client = slot.lock().await;

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

/// List all service organizations the connected user can see.
/// Used by the SO combobox to populate the typeahead. `destination` selects the destination
/// server (migration) instead of the source.
#[tauri::command]
pub async fn list_service_orgs(
    destination: Option<bool>,
    state: State<'_, AppState>,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    list_service_orgs_for(&state, destination.unwrap_or(false)).await
}

async fn list_service_orgs_for(
    state: &AppState,
    destination: bool,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    let slot = if destination {
        &state.dest_client
    } else {
        &state.client
    };
    let client = slot.lock().await;

    let client = match &*client {
        Some(c) => c,
        None => return Err("Not connected".to_string()),
    };

    match client.get_service_orgs().await {
        Ok(sos) => Ok(sos
            .into_iter()
            .map(|so| serde_json::json!({ "id": so.so_id, "name": so.so_name }))
            .collect()),
        Err(e) => Err(format!("Failed to list service orgs: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::mock_app;
    use tauri::Manager;

    #[test]
    fn server_urls_are_cleaned_up() {
        for typed in [
            "nc.example.com",
            "  nc.example.com  ",
            "https://nc.example.com/",
            "HTTPS://NC.example.com",
            "http://nc.example.com",
            "https://nc.example.com/dashboard?x=1#top",
        ] {
            assert_eq!(
                server_url(typed).to_ascii_lowercase(),
                "https://nc.example.com",
                "typed: {typed:?}"
            );
        }
        assert_eq!(
            server_url("nc.example.com:8443"),
            "https://nc.example.com:8443"
        );
    }

    /// A host that refuses connections immediately, so connecting fails without a network.
    const UNREACHABLE: &str = "127.0.0.1:1";

    async fn connected_state() -> AppState {
        let state = AppState::default();
        *state.client.lock().await = Some(NcClient::new("https://old-source.example"));
        *state.source_soap_client.lock().await =
            Some(NcSoapClient::new("https://old-source.example", "old-jwt"));
        *state.dest_client.lock().await = Some(NcClient::new("https://old-dest.example"));
        *state.dest_soap_client.lock().await =
            Some(NcSoapClient::new("https://old-dest.example", "old-jwt"));
        *state.import_context_cache.lock().await = Some(CachedImportContext {
            service_org_id: 50,
            ctx: crate::import::handlers::ImportContext::for_tests(50),
        });
        state
    }

    async fn server_with_orgs(name: &str, id: i64) -> crate::test_support::MockServer {
        let body = serde_json::json!({
            "data": [{ "soId": id, "soName": name }],
            "totalPages": 1
        });
        crate::test_support::start(move |_, _, _| crate::test_support::json_response(body.clone()))
            .await
    }

    #[tokio::test]
    async fn service_orgs_come_from_the_server_the_caller_asked_about() {
        let source = server_with_orgs("Source Org", 913).await;
        let destination = server_with_orgs("Destination Org", 1059).await;
        let state = AppState::default();
        *state.client.lock().await = Some(crate::test_support::authenticated_client(&source).await);
        *state.dest_client.lock().await =
            Some(crate::test_support::authenticated_client(&destination).await);

        let from_source = list_service_orgs_for(&state, false).await.unwrap();
        let from_destination = list_service_orgs_for(&state, true).await.unwrap();

        assert_eq!(from_source[0]["name"], "Source Org");
        assert_eq!(from_destination[0]["name"], "Destination Org");
        assert_eq!(from_destination[0]["id"], 1059);
    }

    #[tokio::test]
    async fn a_destination_org_is_named_by_the_destination_server() {
        let source = crate::test_support::start(|_, _, _| crate::test_support::not_found()).await;
        let destination = crate::test_support::start(|_, path, _| {
            if path == "/api/service-orgs/1059" {
                crate::test_support::json_response(
                    serde_json::json!({ "soId": 1059, "soName": "N-xport Demo Destination" }),
                )
            } else {
                crate::test_support::not_found()
            }
        })
        .await;
        let state = AppState::default();
        *state.client.lock().await = Some(crate::test_support::authenticated_client(&source).await);
        *state.dest_client.lock().await =
            Some(crate::test_support::authenticated_client(&destination).await);

        let info = get_service_org_info_for(&state, 1059, true).await.unwrap();
        assert_eq!(info["name"], "N-xport Demo Destination");
        assert!(get_service_org_info_for(&state, 1059, false).await.is_err());
    }

    #[tokio::test]
    async fn listing_destination_orgs_needs_a_destination_connection() {
        let state = AppState::default();
        *state.client.lock().await = Some(NcClient::new("https://source.example"));
        assert_eq!(
            list_service_orgs_for(&state, true).await.unwrap_err(),
            "Not connected"
        );
    }

    async fn snapshot(state: &AppState) -> (bool, bool, bool, bool, bool) {
        (
            state.client.lock().await.is_some(),
            state.source_soap_client.lock().await.is_some(),
            state.import_context_cache.lock().await.is_some(),
            state.dest_client.lock().await.is_some(),
            state.dest_soap_client.lock().await.is_some(),
        )
    }

    #[tokio::test]
    async fn clearing_the_source_keeps_the_destination_and_drops_the_import_cache() {
        let state = connected_state().await;
        state.clear_source().await;
        assert_eq!(snapshot(&state).await, (false, false, false, true, true));
    }

    #[tokio::test]
    async fn clearing_the_destination_keeps_the_source() {
        let state = connected_state().await;
        state.clear_destination().await;
        assert_eq!(snapshot(&state).await, (true, true, true, false, false));
    }

    #[tokio::test]
    async fn a_failed_source_connection_does_not_leave_the_previous_server_connected() {
        let app = mock_app();
        app.manage(connected_state().await);
        let state = app.state::<AppState>();

        let result = test_connection(UNREACHABLE.into(), "new-jwt".into(), None, state.clone())
            .await
            .expect("command returns a result");

        assert!(!result.success);
        assert_eq!(
            snapshot(&state).await,
            (false, false, false, true, true),
            "source and its import cache are gone; the destination is untouched"
        );
    }

    #[tokio::test]
    async fn a_failed_destination_connection_does_not_leave_the_previous_destination_connected() {
        let app = mock_app();
        app.manage(connected_state().await);
        let state = app.state::<AppState>();

        let result = connect_destination(UNREACHABLE.into(), "new-jwt".into(), None, state.clone())
            .await
            .expect("command returns a result");

        assert!(!result.success);
        assert_eq!(snapshot(&state).await, (true, true, true, false, false));
    }

    #[tokio::test]
    async fn disconnect_clears_both_connections_and_the_import_cache() {
        let app = mock_app();
        app.manage(connected_state().await);
        let state = app.state::<AppState>();

        disconnect(state.clone()).await.unwrap();
        assert_eq!(snapshot(&state).await, (false, false, false, false, false));
    }
}
