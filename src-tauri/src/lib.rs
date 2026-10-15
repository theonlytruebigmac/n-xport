//! N-Central Data Export Tool - Rust Backend Library
//!
//! Cross-platform data export utility for N-Central RMM using the REST API.

pub mod api;
pub mod cli;
pub mod commands;
pub mod config;
pub mod credentials;
pub mod error;
pub mod export;
pub mod import;
pub mod models;
#[cfg(test)]
pub(crate) mod test_support;

pub use error::{AppError, Result};

use commands::connection::AppState;

/// Initialize logging
pub fn init_logging() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();
}

/// Apply the saved credential-storage opt-in, and move any credentials left in the old
/// unencrypted-in-practice file into the keychain. Entries that cannot move yet stay readable
/// where they are and move once encrypted file storage is turned on.
pub fn init_credential_storage() {
    match config::Settings::load() {
        Ok(settings) => {
            credentials::CredentialStore::set_disk_enabled(settings.allow_disk_credentials)
        }
        Err(e) => tracing::warn!("Could not read settings for credential storage: {}", e),
    }
    match credentials::CredentialStore::migrate_legacy_credentials() {
        Ok(report) if report.to_keychain > 0 => {
            tracing::info!("Moved {} credential(s) from the old file into the keychain", report.to_keychain)
        }
        Ok(report) if report.remaining > 0 => tracing::warn!(
            "{} credential(s) are still in the old unencrypted file; turn on encrypted file storage to protect them",
            report.remaining
        ),
        Ok(_) => {}
        Err(e) => tracing::warn!("Could not migrate old credentials: {}", e),
    }
}

/// Create the Tauri application
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    init_credential_storage();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            // Connection commands
            commands::test_connection,
            commands::connect_with_profile,
            commands::save_credentials,
            commands::has_credentials,
            commands::get_credentials,
            commands::get_password,
            commands::delete_credentials,
            commands::get_credential_storage_status,
            commands::enable_disk_credentials,
            commands::unlock_credentials,
            commands::lock_credentials,
            commands::disable_disk_credentials,
            commands::disconnect,
            commands::get_service_org_info,
            commands::list_service_orgs,
            commands::connect_destination,
            // Config commands
            commands::get_settings,
            commands::save_settings,
            commands::get_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::set_active_profile,
            commands::get_active_profile,
            // Export commands
            commands::start_export,
            commands::get_export_types,
            commands::open_directory,
            commands::cancel_export,
            // Import commands
            commands::start_import,
            commands::generate_template,
            commands::get_import_types,
            // Migration commands
            commands::start_migration,
            commands::migration_preflight,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
