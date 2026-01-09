//! Config-related Tauri commands

use crate::config::{Profile, Settings};

/// Get all settings
#[tauri::command]
pub async fn get_settings() -> std::result::Result<Settings, String> {
    Settings::load().map_err(|e| e.to_string())
}

/// Save settings
#[tauri::command]
pub async fn save_settings(settings: Settings) -> std::result::Result<(), String> {
    settings.save().map_err(|e| e.to_string())
}

/// Get all profiles
#[tauri::command]
pub async fn get_profiles() -> std::result::Result<Vec<Profile>, String> {
    let settings = Settings::load().map_err(|e| e.to_string())?;
    Ok(settings.profiles)
}

/// Add or update a profile
#[tauri::command]
pub async fn save_profile(profile: Profile) -> std::result::Result<(), String> {
    let mut settings = Settings::load().map_err(|e| e.to_string())?;
    settings.add_profile(profile);
    settings.save().map_err(|e| e.to_string())
}

/// Delete a profile
#[tauri::command]
pub async fn delete_profile(name: String) -> std::result::Result<(), String> {
    let mut settings = Settings::load().map_err(|e| e.to_string())?;
    settings.delete_profile(&name);
    settings.save().map_err(|e| e.to_string())
}

/// Set the active profile
#[tauri::command]
pub async fn set_active_profile(name: String) -> std::result::Result<(), String> {
    let mut settings = Settings::load().map_err(|e| e.to_string())?;
    settings.set_active_profile(&name).map_err(|e| e.to_string())?;
    settings.save().map_err(|e| e.to_string())
}

/// Get the active profile
#[tauri::command]
pub async fn get_active_profile() -> std::result::Result<Option<Profile>, String> {
    let settings = Settings::load().map_err(|e| e.to_string())?;
    Ok(settings.get_active_profile().cloned())
}
