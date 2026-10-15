//! Credential storage: the OS keychain, with an opt-in passphrase-encrypted file
//!
//! Secrets live in the system keychain:
//! - Windows: Credential Manager
//! - macOS: Keychain
//! - Linux: Secret Service (GNOME Keyring, KWallet)
//!
//! If the keychain is unavailable, nothing is written to disk by default: the credential is
//! kept for the current session only. A user can opt in to an encrypted file instead (see
//! [`super::vault`]); it is protected by a passphrase they choose, and is only readable while
//! unlocked. The older unencrypted-in-practice XOR file is read-only legacy: its entries are
//! moved into the keychain or the encrypted file and the file is deleted.

use super::vault::{Vault, VaultError, MIN_PASSPHRASE_LEN};
use crate::error::{AppError, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use zeroize::Zeroizing;

const SERVICE_NAME: &str = "nc-data-export";

/// Shown when a credential is in the encrypted file but the passphrase has not been entered.
pub const LOCKED_MESSAGE: &str =
    "the encrypted credentials file is locked; unlock it with your passphrase";

/// Where a secret ended up after a store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageBackend {
    /// The operating system keychain.
    Keychain,
    /// The passphrase-encrypted file (the user opted in).
    EncryptedFile,
    /// Not saved anywhere: the keychain failed and encrypted-file storage is off.
    SessionOnly,
}

impl StorageBackend {
    /// A warning to show the user when the secret was not saved to the keychain or vault.
    pub fn warning(self) -> Option<&'static str> {
        match self {
            StorageBackend::Keychain | StorageBackend::EncryptedFile => None,
            StorageBackend::SessionOnly => Some(
                "The system keychain is unavailable, so this credential was not saved and will be \
                 forgotten when the app closes. To remember credentials without a keychain, turn \
                 on encrypted file storage in Credential storage settings.",
            ),
        }
    }
}

/// Whether the encrypted file may be used, and the passphrase once it is unlocked.
#[derive(Default)]
pub struct DiskAccess {
    enabled: bool,
    passphrase: Option<Zeroizing<String>>,
}

pub type SharedDiskAccess = Arc<Mutex<DiskAccess>>;

fn global_disk_access() -> SharedDiskAccess {
    static ACCESS: OnceLock<SharedDiskAccess> = OnceLock::new();
    ACCESS.get_or_init(SharedDiskAccess::default).clone()
}

/// What the UI needs to show about credential storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    /// The user has opted in to the encrypted file.
    pub disk_enabled: bool,
    /// An encrypted file exists on disk.
    pub vault_exists: bool,
    /// The passphrase has been entered this session.
    pub unlocked: bool,
    /// Entries still in the old unencrypted-in-practice file.
    pub legacy_entries: usize,
}

/// What moving the old file's entries did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMigration {
    pub to_keychain: usize,
    pub to_encrypted_file: usize,
    /// Entries that could not be moved yet and are still in the old file.
    pub remaining: usize,
}

fn data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "fraziersystems", "nc-data-export")
        .map(|dirs| dirs.data_dir().to_path_buf())
}

fn vault_err(error: VaultError) -> AppError {
    AppError::Credential(error.to_string())
}

fn locked() -> AppError {
    AppError::Credential(LOCKED_MESSAGE.into())
}

/// The old format's key. Only used to read entries left by earlier versions.
fn legacy_key() -> Vec<u8> {
    format!("{}-jwt-store-key", SERVICE_NAME).into_bytes()
}

fn legacy_xor(data: &[u8]) -> Vec<u8> {
    let key = legacy_key();
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect()
}

fn deobfuscate_legacy(data: &str) -> Option<String> {
    let decoded = BASE64.decode(data).ok()?;
    String::from_utf8(legacy_xor(&decoded)).ok()
}

#[cfg(test)]
fn obfuscate_legacy(data: &str) -> String {
    BASE64.encode(legacy_xor(data.as_bytes()))
}

/// The system keychain operations the store needs. Abstracted so the fallback and deletion
/// behavior can be tested without a real keychain.
trait SecretBackend {
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    fn get(&self, account: &str) -> Result<Option<String>>;
    /// Deleting an account that does not exist is not an error.
    fn delete(&self, account: &str) -> Result<()>;
}

struct OsKeyring;

impl OsKeyring {
    fn entry(account: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE_NAME, account)
            .map_err(|e| AppError::Credential(format!("Failed to create keyring entry: {}", e)))
    }
}

impl SecretBackend for OsKeyring {
    fn set(&self, account: &str, secret: &str) -> Result<()> {
        Self::entry(account)?
            .set_password(secret)
            .map_err(|e| AppError::Credential(format!("Failed to store secret in keyring: {}", e)))
    }

    fn get(&self, account: &str) -> Result<Option<String>> {
        match Self::entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Credential(format!(
                "Failed to retrieve secret from keyring: {}",
                e
            ))),
        }
    }

    fn delete(&self, account: &str) -> Result<()> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Credential(format!(
                "Failed to delete secret from keyring: {}",
                e
            ))),
        }
    }
}

/// Keychain-first secret storage with an opt-in encrypted file.
struct SecretStore<B: SecretBackend> {
    backend: B,
    vault: Vault,
    legacy: Option<PathBuf>,
    disk: SharedDiskAccess,
}

impl<B: SecretBackend> SecretStore<B> {
    fn access(&self) -> std::sync::MutexGuard<'_, DiskAccess> {
        self.disk
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // ---- legacy file (read and remove only) ----

    fn load_legacy(&self) -> HashMap<String, String> {
        self.legacy
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save_legacy(&self, entries: &HashMap<String, String>) -> Result<()> {
        let Some(path) = &self.legacy else {
            return Ok(());
        };
        if entries.is_empty() {
            return match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(AppError::Credential(format!(
                    "Failed to remove the old credentials file: {}",
                    e
                ))),
            };
        }
        let json = serde_json::to_string_pretty(entries)
            .map_err(|e| AppError::Credential(format!("Failed to serialize credentials: {}", e)))?;
        fs::write(path, json).map_err(|e| {
            AppError::Credential(format!("Failed to update the old credentials file: {}", e))
        })
    }

    fn remove_legacy_entry(&self, account: &str) -> Result<()> {
        let mut entries = self.load_legacy();
        if entries.remove(account).is_some() {
            self.save_legacy(&entries)?;
        }
        Ok(())
    }

    // ---- store / get / delete ----

    /// Write to the keychain and read it back; some keychains accept writes and drop them.
    fn keychain_store(&self, account: &str, secret: &str) -> Result<()> {
        self.backend.set(account, secret)?;
        match self.backend.get(account)? {
            Some(stored) if stored == secret => Ok(()),
            _ => Err(AppError::Credential(
                "Keychain accepted the secret but did not return it".into(),
            )),
        }
    }

    /// Store a secret, preferring the keychain. The result says where it actually went;
    /// callers must surface [`StorageBackend::warning`] to the user.
    fn store(&self, account: &str, secret: &str) -> Result<StorageBackend> {
        match self.keychain_store(account, secret) {
            Ok(()) => {
                // The keychain is authoritative now; leave no older copy on disk.
                for cleanup in [
                    self.vault.remove(account).map_err(vault_err),
                    self.remove_legacy_entry(account),
                ] {
                    if let Err(e) = cleanup {
                        tracing::warn!(
                            "Could not remove an older stored copy for '{}': {}",
                            account,
                            e
                        );
                    }
                }
                tracing::info!("Secret for '{}' stored in the system keychain", account);
                Ok(StorageBackend::Keychain)
            }
            Err(keychain_error) => {
                tracing::warn!("Keychain unavailable for '{}': {}", account, keychain_error);
                // Reads prefer the keychain, so an older keychain value would shadow the one
                // being saved. Remove it; if that fails, say so rather than return stale data.
                self.backend.delete(account).map_err(|e| {
                    AppError::Credential(format!(
                        "Could not save the credential: the system keychain failed ({}) and its \
                         existing entry could not be removed ({}), so it would shadow the new value",
                        keychain_error, e
                    ))
                })?;

                let passphrase = {
                    let access = self.access();
                    if !access.enabled {
                        None
                    } else {
                        Some(access.passphrase.clone().ok_or_else(locked)?)
                    }
                };
                // Whatever happens next, an older copy of this credential must not survive.
                self.remove_legacy_entry(account)?;
                match passphrase {
                    // Encrypted file storage is off: keep the credential for this session only.
                    None => {
                        self.vault.remove(account).map_err(vault_err)?;
                        Ok(StorageBackend::SessionOnly)
                    }
                    Some(passphrase) => {
                        self.vault
                            .put(&passphrase, account, secret)
                            .map_err(vault_err)?;
                        Ok(StorageBackend::EncryptedFile)
                    }
                }
            }
        }
    }

    /// Whether any store holds this account, without needing the passphrase.
    fn has(&self, account: &str) -> bool {
        matches!(self.backend.get(account), Ok(Some(_)))
            || self.vault.contains(account)
            || self.load_legacy().contains_key(account)
    }

    /// Retrieve a secret: keychain, then the encrypted file (if unlocked), then the legacy file.
    fn get(&self, account: &str) -> Result<Option<String>> {
        match self.backend.get(account) {
            Ok(Some(secret)) => return Ok(Some(secret)),
            Ok(None) => {}
            Err(e) => tracing::warn!("Keychain read failed for '{}': {}", account, e),
        }
        if self.vault.contains(account) {
            let passphrase = {
                let access = self.access();
                if access.enabled {
                    access.passphrase.clone()
                } else {
                    None
                }
            };
            let passphrase = passphrase.ok_or_else(locked)?;
            return Ok(self
                .vault
                .get(&passphrase, account)
                .map_err(vault_err)?
                .map(|secret| secret.to_string()));
        }
        Ok(self
            .load_legacy()
            .get(account)
            .and_then(|obfuscated| deobfuscate_legacy(obfuscated)))
    }

    /// Delete a secret from every store. Fails if any copy could not be removed, so a caller
    /// never believes a secret is gone when it is not.
    fn delete(&self, account: &str) -> Result<()> {
        let failures: Vec<String> = [
            self.backend
                .delete(account)
                .err()
                .map(|e| format!("the system keychain: {}", e)),
            self.vault
                .remove(account)
                .err()
                .map(|e| format!("the encrypted credentials file: {}", e)),
            self.remove_legacy_entry(account)
                .err()
                .map(|e| format!("the old credentials file: {}", e)),
        ]
        .into_iter()
        .flatten()
        .collect();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Credential(format!(
                "'{}' could not be removed from {}",
                account,
                failures.join(" or ")
            )))
        }
    }

    /// Delete every secret kept for a profile (source and destination JWT and password),
    /// attempting all of them and reporting every failure.
    fn delete_profile(&self, profile_name: &str) -> Result<()> {
        let dest = format!("{}_dest", profile_name);
        let accounts = [
            profile_name.to_string(),
            format!("{}_password", profile_name),
            dest.clone(),
            format!("{}_password", dest),
        ];
        let failures: Vec<String> = accounts
            .iter()
            .filter_map(|account| self.delete(account).err().map(|e| e.to_string()))
            .collect();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Credential(failures.join("; ")))
        }
    }

    // ---- encrypted-file opt-in ----

    pub fn status(&self) -> StorageStatus {
        let access = self.access();
        StorageStatus {
            disk_enabled: access.enabled,
            vault_exists: self.vault.exists(),
            unlocked: access.enabled && access.passphrase.is_some(),
            legacy_entries: self.load_legacy().len(),
        }
    }

    /// Turn on encrypted file storage with this passphrase. A new file needs a passphrase of
    /// at least [`MIN_PASSPHRASE_LEN`] characters; an existing one must be opened with its own.
    fn enable_disk(&self, passphrase: &str) -> Result<LegacyMigration> {
        if self.vault.exists() {
            self.vault.verify(passphrase).map_err(vault_err)?;
        } else if passphrase.chars().count() < MIN_PASSPHRASE_LEN {
            return Err(vault_err(VaultError::PassphraseTooShort));
        }
        {
            let mut access = self.access();
            access.enabled = true;
            access.passphrase = Some(Zeroizing::new(passphrase.to_string()));
        }
        self.migrate_legacy()
    }

    /// Enter the passphrase for the encrypted file. Encrypted storage must already be on.
    fn unlock(&self, passphrase: &str) -> Result<()> {
        if !self.access().enabled {
            return Err(AppError::Credential(
                "encrypted file storage is not turned on".into(),
            ));
        }
        self.vault.verify(passphrase).map_err(vault_err)?;
        self.access().passphrase = Some(Zeroizing::new(passphrase.to_string()));
        Ok(())
    }

    fn lock(&self) {
        self.access().passphrase = None;
    }

    /// Turn encrypted file storage off. The file is kept (and stays unreadable) unless
    /// `delete_file` is set, which is also how a forgotten passphrase is reset.
    fn disable_disk(&self, delete_file: bool) -> Result<()> {
        {
            let mut access = self.access();
            access.enabled = false;
            access.passphrase = None;
        }
        if delete_file {
            self.vault.delete().map_err(vault_err)?;
        }
        Ok(())
    }

    /// Move entries out of the old file: into the keychain if it works, otherwise into the
    /// encrypted file if that is on and unlocked. Entries that cannot move yet stay readable
    /// in the old file so nothing is lost.
    fn migrate_legacy(&self) -> Result<LegacyMigration> {
        let mut entries = self.load_legacy();
        let mut report = LegacyMigration::default();
        let passphrase = {
            let access = self.access();
            if access.enabled {
                access.passphrase.clone()
            } else {
                None
            }
        };
        let accounts: Vec<String> = entries.keys().cloned().collect();
        for account in accounts {
            let Some(secret) = entries.get(&account).and_then(|o| deobfuscate_legacy(o)) else {
                report.remaining += 1;
                continue;
            };
            if self.keychain_store(&account, &secret).is_ok() {
                entries.remove(&account);
                report.to_keychain += 1;
            } else if let Some(passphrase) = &passphrase {
                if self.vault.put(passphrase, &account, &secret).is_ok() {
                    entries.remove(&account);
                    report.to_encrypted_file += 1;
                } else {
                    report.remaining += 1;
                }
            } else {
                report.remaining += 1;
            }
        }
        if report.to_keychain + report.to_encrypted_file > 0 {
            self.save_legacy(&entries)?;
        }
        Ok(report)
    }
}

fn default_store() -> SecretStore<OsKeyring> {
    let dir = data_dir();
    SecretStore {
        backend: OsKeyring,
        vault: Vault::new(
            dir.clone()
                .map(|d| d.join("credentials.vault.json"))
                .unwrap_or_else(|| PathBuf::from("credentials.vault.json")),
        ),
        legacy: dir.map(|d| d.join("credentials.json")),
        disk: global_disk_access(),
    }
}

/// Credential manager: the keychain first, an opt-in encrypted file second.
pub struct CredentialStore;

impl CredentialStore {
    /// Store a JWT for a profile. The result says whether it reached the keychain, the
    /// encrypted file, or was kept for this session only.
    pub fn store_jwt(profile_name: &str, jwt: &str) -> Result<StorageBackend> {
        default_store().store(profile_name, jwt)
    }

    /// Retrieve a JWT for a profile (keychain, then the encrypted file if unlocked)
    pub fn get_jwt(profile_name: &str) -> Result<Option<String>> {
        default_store().get(profile_name)
    }

    /// Delete a JWT for a profile from every store
    pub fn delete_jwt(profile_name: &str) -> Result<()> {
        default_store().delete(profile_name)
    }

    /// Check if a JWT exists for a profile, without needing the passphrase
    pub fn has_jwt(profile_name: &str) -> bool {
        default_store().has(profile_name)
    }

    /// Store a password for a profile
    pub fn store_password(profile_name: &str, password: &str) -> Result<StorageBackend> {
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

    /// Delete every credential stored for a profile, including its destination
    /// credentials, from every store.
    pub fn delete_profile_credentials(profile_name: &str) -> Result<()> {
        default_store().delete_profile(profile_name)
    }

    /// The saved opt-in, applied at startup. Does not unlock anything.
    pub fn set_disk_enabled(enabled: bool) {
        let access = global_disk_access();
        let mut access = access.lock().unwrap_or_else(|p| p.into_inner());
        access.enabled = enabled;
        if !enabled {
            access.passphrase = None;
        }
    }

    pub fn storage_status() -> StorageStatus {
        default_store().status()
    }

    pub fn enable_disk_storage(passphrase: &str) -> Result<LegacyMigration> {
        default_store().enable_disk(passphrase)
    }

    pub fn unlock(passphrase: &str) -> Result<()> {
        default_store().unlock(passphrase)
    }

    pub fn lock() {
        default_store().lock()
    }

    pub fn disable_disk_storage(delete_file: bool) -> Result<()> {
        default_store().disable_disk(delete_file)
    }

    /// Move entries out of the old unencrypted-in-practice file where possible.
    pub fn migrate_legacy_credentials() -> Result<LegacyMigration> {
        default_store().migrate_legacy()
    }
}

#[cfg(test)]
mod tests;
