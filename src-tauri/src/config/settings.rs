//! Application settings and profiles

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{AppError, Result};

/// Profile type: export (single connection) or migration (dual connection)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProfileType {
    Export,
    Migration,
}

impl Default for ProfileType {
    fn default() -> Self {
        ProfileType::Export
    }
}

/// A single connection configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionConfig {
    /// N-Central server FQDN
    pub fqdn: String,
    /// API Username (Required for SOAP)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Target Service Organization ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_org_id: Option<i64>,
}

/// A server profile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Profile name (e.g., "Production", "Staging")
    pub name: String,
    /// Profile type (export or migration)
    #[serde(default, rename = "type")]
    pub profile_type: ProfileType,
    /// Source connection config
    pub source: ConnectionConfig,
    /// Destination connection config (only for migration type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<ConnectionConfig>,
    /// Last used timestamp
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<String>,
}

impl Profile {
    /// Create a new export profile
    pub fn new_export(name: &str, fqdn: &str) -> Self {
        Self {
            name: name.to_string(),
            profile_type: ProfileType::Export,
            source: ConnectionConfig {
                fqdn: fqdn.to_string(),
                username: None,
                service_org_id: None,
            },
            destination: None,
            last_used: None,
        }
    }

    /// Create a new migration profile
    pub fn new_migration(name: &str, source_fqdn: &str, dest_fqdn: &str) -> Self {
        Self {
            name: name.to_string(),
            profile_type: ProfileType::Migration,
            source: ConnectionConfig {
                fqdn: source_fqdn.to_string(),
                username: None,
                service_org_id: None,
            },
            destination: Some(ConnectionConfig {
                fqdn: dest_fqdn.to_string(),
                username: None,
                service_org_id: None,
            }),
            last_used: None,
        }
    }

    /// Get the base URL for API requests (source)
    pub fn base_url(&self) -> String {
        format!("https://{}", self.source.fqdn)
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
        self.active_profile
            .as_ref()
            .and_then(|name| self.profiles.iter().find(|p| &p.name == name))
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
        if self
            .active_profile
            .as_ref()
            .map(|n| n == name)
            .unwrap_or(false)
        {
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
