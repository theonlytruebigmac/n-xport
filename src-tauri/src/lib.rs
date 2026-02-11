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
pub mod models;

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

/// Create the Tauri application
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();

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
            commands::disconnect,
            commands::get_service_org_info,
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
            // Migration commands
            commands::start_migration,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
