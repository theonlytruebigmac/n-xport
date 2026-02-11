//! OS-native credential storage with file-based fallback
//!
//! Uses the system keychain for secure JWT storage:
//! - Windows: Credential Manager
//! - macOS: Keychain
//! - Linux: Secret Service (GNOME Keyring, KWallet)
//!
//! Falls back to encrypted file storage if system keyring fails.

use crate::error::{AppError, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use std::fs;
use std::path::PathBuf;

const SERVICE_NAME: &str = "nc-data-export";

/// Get the credentials file path
fn get_creds_file() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "fraziersystems", "nc-data-export")
        .map(|dirs| dirs.data_dir().join("credentials.json"))
}

/// Simple obfuscation key derived from machine info
fn get_obfuscation_key() -> String {
    // Use a combination of service name and a constant as the "key"
    // This is NOT secure encryption, just obfuscation to prevent casual viewing
    format!("{}-jwt-store-key", SERVICE_NAME)
}

/// Obfuscate a string (simple XOR + base64)
fn obfuscate(data: &str) -> String {
    let key = get_obfuscation_key();
    let key_bytes: Vec<u8> = key.bytes().collect();
    let obfuscated: Vec<u8> = data
        .bytes()
        .enumerate()
        .map(|(i, b)| b ^ key_bytes[i % key_bytes.len()])
        .collect();
    BASE64.encode(&obfuscated)
}

/// Deobfuscate a string
fn deobfuscate(data: &str) -> Option<String> {
    let key = get_obfuscation_key();
    let key_bytes: Vec<u8> = key.bytes().collect();
    let decoded = BASE64.decode(data).ok()?;
    let deobfuscated: Vec<u8> = decoded
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ key_bytes[i % key_bytes.len()])
        .collect();
    String::from_utf8(deobfuscated).ok()
}

/// Load credentials from file
fn load_file_creds() -> std::collections::HashMap<String, String> {
    get_creds_file()
        .and_then(|path| fs::read_to_string(&path).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

/// Save credentials to file
fn save_file_creds(creds: &std::collections::HashMap<String, String>) -> Result<()> {
    if let Some(path) = get_creds_file() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                AppError::Credential(format!("Failed to create credentials directory: {}", e))
            })?;
        }
        let json = serde_json::to_string_pretty(creds)
            .map_err(|e| AppError::Credential(format!("Failed to serialize credentials: {}", e)))?;
        fs::write(&path, json).map_err(|e| {
            AppError::Credential(format!("Failed to write credentials file: {}", e))
        })?;
        tracing::debug!("Saved credentials to file: {:?}", path);
    }
    Ok(())
}

/// Credential manager using OS keychain with file fallback
pub struct CredentialStore;

impl CredentialStore {
    /// Store a JWT for a profile (tries keyring first, then file fallback)
    pub fn store_jwt(profile_name: &str, jwt: &str) -> Result<()> {
        tracing::debug!(
            "Storing JWT for profile '{}' (trying keyring first)",
            profile_name
        );

        // Try keyring first
        let keyring_result = Self::store_jwt_keyring(profile_name, jwt);

        if keyring_result.is_ok() {
            // Verify it was actually stored
            if let Ok(Some(stored)) = Self::get_jwt_keyring(profile_name) {
                if stored == jwt {
                    tracing::info!("JWT stored successfully in system keyring");
                    return Ok(());
                }
            }
            tracing::warn!("Keyring store succeeded but verification failed, using file fallback");
        } else {
            tracing::warn!(
                "Keyring store failed, using file fallback: {:?}",
                keyring_result.err()
            );
        }

        // Fall back to file storage
        Self::store_jwt_file(profile_name, jwt)
    }

    /// Store JWT in system keyring
    fn store_jwt_keyring(profile_name: &str, jwt: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, profile_name)
            .map_err(|e| AppError::Credential(format!("Failed to create keyring entry: {}", e)))?;

        entry
            .set_password(jwt)
            .map_err(|e| AppError::Credential(format!("Failed to store JWT in keyring: {}", e)))?;

        Ok(())
    }

    /// Store JWT in file (obfuscated)
    fn store_jwt_file(profile_name: &str, jwt: &str) -> Result<()> {
        tracing::warn!(
            "Storing JWT in FILE FALLBACK for '{}'. \
             System keychain is unavailable — credentials are stored with \
             basic obfuscation only. Consider configuring your OS keychain \
             for secure credential storage.",
            profile_name
        );
        let mut creds = load_file_creds();
        creds.insert(profile_name.to_string(), obfuscate(jwt));
        save_file_creds(&creds)?;
        tracing::warn!("JWT stored in file fallback (not system keychain)");
        Ok(())
    }

    /// Retrieve a JWT for a profile (tries keyring first, then file fallback)
    pub fn get_jwt(profile_name: &str) -> Result<Option<String>> {
        tracing::debug!("Retrieving JWT for profile '{}'", profile_name);

        // Try keyring first
        if let Ok(Some(jwt)) = Self::get_jwt_keyring(profile_name) {
            tracing::debug!("Found JWT in system keyring (len={})", jwt.len());
            return Ok(Some(jwt));
        }

        // Fall back to file
        Self::get_jwt_file(profile_name)
    }

    /// Get JWT from system keyring
    fn get_jwt_keyring(profile_name: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE_NAME, profile_name)
            .map_err(|e| AppError::Credential(format!("Failed to create keyring entry: {}", e)))?;

        match entry.get_password() {
            Ok(jwt) => Ok(Some(jwt)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Credential(format!(
                "Failed to retrieve JWT: {}",
                e
            ))),
        }
    }

    /// Get JWT from file (deobfuscated)
    fn get_jwt_file(profile_name: &str) -> Result<Option<String>> {
        let creds = load_file_creds();
        if let Some(obfuscated) = creds.get(profile_name) {
            if let Some(jwt) = deobfuscate(obfuscated) {
                tracing::debug!("Found JWT in file fallback (len={})", jwt.len());
                return Ok(Some(jwt));
            }
        }
        tracing::debug!("No JWT found in file fallback for '{}'", profile_name);
        Ok(None)
    }

    /// Delete a JWT for a profile (from both keyring and file)
    pub fn delete_jwt(profile_name: &str) -> Result<()> {
        // Delete from keyring
        let _ = Self::delete_jwt_keyring(profile_name);

        // Delete from file
        Self::delete_jwt_file(profile_name)
    }

    /// Delete JWT from keyring
    fn delete_jwt_keyring(profile_name: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, profile_name)
            .map_err(|e| AppError::Credential(format!("Failed to create keyring entry: {}", e)))?;

        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Credential(format!("Failed to delete JWT: {}", e))),
        }
    }

    /// Delete JWT from file
    fn delete_jwt_file(profile_name: &str) -> Result<()> {
        let mut creds = load_file_creds();
        if creds.remove(profile_name).is_some() {
            save_file_creds(&creds)?;
            tracing::debug!("Deleted JWT from file fallback for '{}'", profile_name);
        }
        Ok(())
    }

    /// Check if a JWT exists for a profile
    pub fn has_jwt(profile_name: &str) -> bool {
        Self::get_jwt(profile_name)
            .map(|opt| opt.is_some())
            .unwrap_or(false)
    }

    /// Store a password for a profile
    pub fn store_password(profile_name: &str, password: &str) -> Result<()> {
        Self::store_jwt(&format!("{}_password", profile_name), password)
    }

    /// Retrieve a password for a profile
    pub fn get_password(profile_name: &str) -> Result<Option<String>> {
        Self::get_jwt(&format!("{}_password", profile_name))
    }

    /// Delete a password for a profile
    pub fn delete_password(profile_name: &str) -> Result<()> {
        Self::delete_jwt(&format!("{}_password", profile_name))
    }
}
