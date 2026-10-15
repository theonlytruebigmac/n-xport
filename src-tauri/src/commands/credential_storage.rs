//! Commands for the opt-in encrypted credential file.

use crate::config::Settings;
use crate::credentials::{CredentialStore, LegacyMigration, StorageStatus};

/// Persist the opt-in so it survives a restart. Fails (and the caller undoes the change) if
/// the setting cannot be saved, so the saved state and the running state never disagree.
fn save_opt_in(enabled: bool) -> std::result::Result<(), String> {
    let mut settings = Settings::load().map_err(|e| e.to_string())?;
    settings.allow_disk_credentials = enabled;
    settings.save().map_err(|e| e.to_string())
}

/// Whether encrypted file storage is on, whether a file exists, and whether it is unlocked.
#[tauri::command]
pub async fn get_credential_storage_status() -> StorageStatus {
    CredentialStore::storage_status()
}

/// Turn on encrypted file storage with a passphrase (at least 12 characters for a new file;
/// the existing passphrase for an existing one). Moves entries out of the old unencrypted
/// file into it.
#[tauri::command]
pub async fn enable_disk_credentials(
    passphrase: String,
) -> std::result::Result<LegacyMigration, String> {
    let report = CredentialStore::enable_disk_storage(&passphrase).map_err(|e| e.to_string())?;
    if let Err(e) = save_opt_in(true) {
        let _ = CredentialStore::disable_disk_storage(false);
        return Err(format!("Could not save the setting: {}", e));
    }
    Ok(report)
}

/// Enter the passphrase for an existing encrypted file.
#[tauri::command]
pub async fn unlock_credentials(passphrase: String) -> std::result::Result<(), String> {
    CredentialStore::unlock(&passphrase).map_err(|e| e.to_string())
}

/// Forget the passphrase for this session. Credentials in the file become unreadable until
/// it is entered again.
#[tauri::command]
pub async fn lock_credentials() {
    CredentialStore::lock();
}

/// Turn encrypted file storage off. The file stays on disk, unreadable, unless `delete_file`
/// is set, which is also how a forgotten passphrase is reset.
#[tauri::command]
pub async fn disable_disk_credentials(delete_file: bool) -> std::result::Result<(), String> {
    CredentialStore::disable_disk_storage(delete_file).map_err(|e| e.to_string())?;
    save_opt_in(false)
}
