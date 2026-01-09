//! Application settings and profiles

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;

use crate::error::{AppError, Result};

/// A server profile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Profile name (e.g., "Production", "Staging")
    pub name: String,
    /// N-Central server FQDN
    pub fqdn: String,
    /// Target Service Organization ID
    pub service_org_id: Option<i64>,
    /// Last used timestamp
    #[serde(default)]
    pub last_used: Option<String>,
    /// Encrypted JWT fallback (for when system keyring fails)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_jwt: Option<String>,
}

// Simple encryption key - not secure against determined attackers but protects against casual snooping
const SECRET_KEY: &[u8] = b"NcDataExportTool_SecretKey_Fallback";

impl Profile {
    /// Create a new profile
    pub fn new(name: &str, fqdn: &str) -> Self {
        Self {
            name: name.to_string(),
            fqdn: fqdn.to_string(),
            service_org_id: None,
            last_used: None,
            encrypted_jwt: None,
        }
    }

    /// Encrypt a string (XOR + Base64)
    pub fn encrypt(data: &str) -> String {
        use base64::Engine;
        let bytes: Vec<u8> = data.bytes()
            .zip(SECRET_KEY.iter().cycle())
            .map(|(b, k)| b ^ k)
            .collect();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Decrypt a string
    pub fn decrypt(data: &str) -> std::result::Result<String, String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(data)
            .map_err(|e| e.to_string())?;
        let decrypted: Vec<u8> = bytes.iter()
            .zip(SECRET_KEY.iter().cycle())
            .map(|(b, k)| b ^ k)
            .collect();
        String::from_utf8(decrypted).map_err(|e| e.to_string())
    }

    /// Get the base URL for API requests
    pub fn base_url(&self) -> String {
        format!("https://{}", self.fqdn)
    }
}

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// List of saved profiles
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// Name of the active profile
    pub active_profile: Option<String>,
    /// Default export directory
    pub export_directory: Option<String>,
    /// Default export format
    #[serde(default)]
    pub export_formats: Vec<String>,
    /// Window state
    #[serde(default)]
    pub window: WindowState,
}

/// Window state for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 950,
            height: 800,
            x: None,
            y: None,
            maximized: false,
        }
    }
}

impl Settings {
    /// Get the config file path
    pub fn config_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("com", "fraziersystems", "nc-data-export")
            .ok_or_else(|| AppError::Config("Could not determine config directory".into()))?;
        
        Ok(dirs.config_dir().join("settings.json"))
    }

    /// Load settings from disk
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)?;
        let settings: Settings = serde_json::from_str(&content)?;
        Ok(settings)
    }

    /// Save settings to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        
        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Get the active profile
    pub fn get_active_profile(&self) -> Option<&Profile> {
        self.active_profile.as_ref().and_then(|name| {
            self.profiles.iter().find(|p| &p.name == name)
        })
    }

    /// Get a mutable reference to the active profile
    pub fn get_active_profile_mut(&mut self) -> Option<&mut Profile> {
        let name = self.active_profile.clone()?;
        self.profiles.iter_mut().find(|p| p.name == name)
    }

    /// Add a new profile
    pub fn add_profile(&mut self, profile: Profile) {
        // Remove existing profile with same name
        self.profiles.retain(|p| p.name != profile.name);
        self.profiles.push(profile);
    }

    /// Delete a profile by name
    pub fn delete_profile(&mut self, name: &str) {
        self.profiles.retain(|p| p.name != name);
        
        // Clear active profile if it was deleted
        if self.active_profile.as_ref().map(|n| n == name).unwrap_or(false) {
            self.active_profile = self.profiles.first().map(|p| p.name.clone());
        }
    }

    /// Set the active profile
    pub fn set_active_profile(&mut self, name: &str) -> Result<()> {
        if !self.profiles.iter().any(|p| p.name == name) {
            return Err(AppError::Config(format!("Profile '{}' not found", name)));
        }
        self.active_profile = Some(name.to_string());
        Ok(())
    }
}
