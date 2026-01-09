//! OS-native credential storage
//!
//! Uses the system keychain for secure JWT storage:
//! - Windows: Credential Manager
//! - macOS: Keychain
//! - Linux: Secret Service (GNOME Keyring, KWallet)

use crate::error::{AppError, Result};

const SERVICE_NAME: &str = "nc-data-export";

/// Credential manager using OS keychain
pub struct CredentialStore;

impl CredentialStore {
    /// Store a JWT for a profile
    pub fn store_jwt(profile_name: &str, jwt: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, profile_name)
            .map_err(|e| AppError::Credential(format!("Failed to create keyring entry: {}", e)))?;
        
        entry.set_password(jwt)
            .map_err(|e| AppError::Credential(format!("Failed to store JWT: {}", e)))?;
        
        Ok(())
    }

    /// Retrieve a JWT for a profile
    pub fn get_jwt(profile_name: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE_NAME, profile_name)
            .map_err(|e| AppError::Credential(format!("Failed to create keyring entry: {}", e)))?;
        
        match entry.get_password() {
            Ok(jwt) => Ok(Some(jwt)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Credential(format!("Failed to retrieve JWT: {}", e))),
        }
    }

    /// Delete a JWT for a profile
    pub fn delete_jwt(profile_name: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, profile_name)
            .map_err(|e| AppError::Credential(format!("Failed to create keyring entry: {}", e)))?;
        
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
            Err(e) => Err(AppError::Credential(format!("Failed to delete JWT: {}", e))),
        }
    }

    /// Check if a JWT exists for a profile
    pub fn has_jwt(profile_name: &str) -> bool {
        Self::get_jwt(profile_name)
            .map(|opt| opt.is_some())
            .unwrap_or(false)
    }
}
