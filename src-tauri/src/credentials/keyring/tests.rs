use super::super::vault::KdfParams;
use super::*;
use std::cell::RefCell;
use tempfile::TempDir;

const PASSPHRASE: &str = "correct horse battery";

/// In-memory keychain whose operations can be made to fail.
#[derive(Default)]
struct FakeKeychain {
    secrets: RefCell<HashMap<String, String>>,
    fail_set: bool,
    fail_get: bool,
    fail_delete: bool,
    /// Accepts writes but never remembers them.
    forgetful: bool,
}

impl SecretBackend for FakeKeychain {
    fn set(&self, account: &str, secret: &str) -> Result<()> {
        if self.fail_set {
            return Err(AppError::Credential("keychain locked".into()));
        }
        if !self.forgetful {
            self.secrets
                .borrow_mut()
                .insert(account.into(), secret.into());
        }
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>> {
        if self.fail_get {
            return Err(AppError::Credential("keychain locked".into()));
        }
        Ok(self.secrets.borrow().get(account).cloned())
    }
    fn delete(&self, account: &str) -> Result<()> {
        if self.fail_delete {
            return Err(AppError::Credential("keychain locked".into()));
        }
        self.secrets.borrow_mut().remove(account);
        Ok(())
    }
}

fn store(keychain: FakeKeychain) -> (SecretStore<FakeKeychain>, TempDir) {
    let dir = TempDir::new().expect("temp dir");
    let vault = Vault::with_params(
        dir.path().join("credentials.vault.json"),
        KdfParams {
            memory_kib: 8,
            iterations: 1,
            parallelism: 1,
        },
    );
    let store = SecretStore {
        backend: keychain,
        vault,
        legacy: Some(dir.path().join("credentials.json")),
        disk: Arc::new(Mutex::new(DiskAccess::default())),
    };
    (store, dir)
}

fn failing() -> FakeKeychain {
    FakeKeychain {
        fail_set: true,
        ..Default::default()
    }
}

fn put_legacy(store: &SecretStore<FakeKeychain>, account: &str, secret: &str) {
    let mut entries = store.load_legacy();
    entries.insert(account.into(), obfuscate_legacy(secret));
    fs::write(
        store.legacy.as_ref().unwrap(),
        serde_json::to_string(&entries).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_working_keychain_holds_the_secret_and_nothing_is_written_to_disk() {
    let (store, dir) = store(FakeKeychain::default());
    assert_eq!(
        store.store("acme", "jwt-1").unwrap(),
        StorageBackend::Keychain
    );
    assert_eq!(store.get("acme").unwrap().as_deref(), Some("jwt-1"));
    assert!(!store.vault.exists());
    assert!(!dir.path().join("credentials.json").exists());
    assert!(StorageBackend::Keychain.warning().is_none());
    assert!(StorageBackend::EncryptedFile.warning().is_none());
}

#[test]
fn with_encrypted_storage_off_a_failing_keychain_keeps_the_credential_for_the_session_only() {
    let (store, dir) = store(failing());
    let backend = store.store("acme", "jwt-1").unwrap();
    assert_eq!(backend, StorageBackend::SessionOnly);
    let warning = backend.warning().unwrap();
    assert!(
        warning.contains("forgotten when the app closes")
            && warning.contains("encrypted file storage")
    );

    // Nothing at all was written.
    assert!(!store.vault.exists());
    assert!(!dir.path().join("credentials.json").exists());
    assert_eq!(store.get("acme").unwrap(), None);
    assert!(!store.has("acme"));
}

#[test]
fn a_keychain_that_forgets_writes_is_treated_as_unavailable() {
    let (store, _dir) = store(FakeKeychain {
        forgetful: true,
        ..Default::default()
    });
    assert_eq!(
        store.store("acme", "jwt-1").unwrap(),
        StorageBackend::SessionOnly
    );
}

#[test]
fn opted_in_and_unlocked_a_failing_keychain_falls_back_to_the_encrypted_file() {
    let (store, _dir) = store(failing());
    store.enable_disk(PASSPHRASE).unwrap();

    assert_eq!(
        store.store("acme", "super-secret-jwt").unwrap(),
        StorageBackend::EncryptedFile
    );
    assert_eq!(
        store.get("acme").unwrap().as_deref(),
        Some("super-secret-jwt")
    );
    let on_disk = fs::read_to_string(store.vault.path()).unwrap();
    assert!(!on_disk.contains("super-secret-jwt"));
    assert!(store.has("acme"));
}

#[test]
fn opted_in_but_locked_nothing_is_written_and_the_user_is_told_to_unlock() {
    let (store, _dir) = store(failing());
    store.enable_disk(PASSPHRASE).unwrap();
    store.lock();

    let error = store.store("acme", "jwt").unwrap_err().to_string();
    assert!(error.contains("locked"), "{}", error);
    assert!(!store.vault.exists());
}

#[test]
fn a_locked_vault_still_reports_that_a_credential_exists_and_needs_the_passphrase_to_read_it() {
    let (store, _dir) = store(failing());
    store.enable_disk(PASSPHRASE).unwrap();
    store.store("acme", "jwt-1").unwrap();

    store.lock();
    assert!(store.has("acme"), "existence needs no passphrase");
    assert!(store
        .get("acme")
        .unwrap_err()
        .to_string()
        .contains("locked"));

    assert!(store.unlock("the wrong passphrase").is_err());
    assert!(
        store.get("acme").is_err(),
        "a failed unlock leaves it locked"
    );
    store.unlock(PASSPHRASE).unwrap();
    assert_eq!(store.get("acme").unwrap().as_deref(), Some("jwt-1"));
    assert!(store.status().unlocked);
}

#[test]
fn turning_the_option_off_makes_the_file_unreadable_until_it_is_turned_back_on() {
    let (store, _dir) = store(failing());
    store.enable_disk(PASSPHRASE).unwrap();
    store.store("acme", "jwt-1").unwrap();

    store.disable_disk(false).unwrap();
    assert!(store.vault.exists(), "the file is kept");
    assert!(
        store.get("acme").is_err(),
        "but cannot be read while the option is off"
    );
    assert!(
        store.unlock(PASSPHRASE).is_err(),
        "unlock needs the option turned on first"
    );

    store.enable_disk(PASSPHRASE).unwrap();
    assert_eq!(store.get("acme").unwrap().as_deref(), Some("jwt-1"));

    store.disable_disk(true).unwrap();
    assert!(
        !store.vault.exists(),
        "deleting the file resets a forgotten passphrase"
    );
    assert!(!store.status().disk_enabled);
}

#[test]
fn enabling_needs_a_long_enough_passphrase_for_a_new_file_and_the_right_one_for_an_existing_file() {
    let (store, _dir) = store(failing());
    assert!(store
        .enable_disk("too short")
        .unwrap_err()
        .to_string()
        .contains("at least 12"));
    assert!(!store.status().disk_enabled);

    store.enable_disk(PASSPHRASE).unwrap();
    store.store("acme", "jwt").unwrap();
    store.disable_disk(false).unwrap();
    assert!(store
        .enable_disk("a different passphrase")
        .unwrap_err()
        .to_string()
        .contains("wrong passphrase"));
    assert!(
        !store.status().disk_enabled,
        "a wrong passphrase must not turn it on"
    );
}

#[test]
fn falling_back_never_leaves_an_older_keychain_value_to_shadow_the_new_one() {
    let keychain = FakeKeychain::default();
    keychain
        .secrets
        .borrow_mut()
        .insert("acme".into(), "old-jwt".into());
    let (mut store, _dir) = store(keychain);
    store.backend.fail_set = true;
    store.enable_disk(PASSPHRASE).unwrap();

    assert_eq!(
        store.store("acme", "new-jwt").unwrap(),
        StorageBackend::EncryptedFile
    );
    assert_eq!(store.get("acme").unwrap().as_deref(), Some("new-jwt"));
}

#[test]
fn a_fallback_save_fails_loudly_if_the_old_keychain_value_cannot_be_removed() {
    let keychain = FakeKeychain::default();
    keychain
        .secrets
        .borrow_mut()
        .insert("acme".into(), "old-jwt".into());
    let (mut store, _dir) = store(keychain);
    store.backend.fail_set = true;
    store.backend.fail_delete = true;
    store.enable_disk(PASSPHRASE).unwrap();

    assert!(store.store("acme", "new-jwt").is_err());
    assert!(
        !store.vault.exists(),
        "nothing is written when it would be shadowed"
    );
}

#[test]
fn saving_to_a_recovered_keychain_removes_stale_encrypted_and_legacy_copies() {
    let (mut store, _dir) = store(failing());
    store.enable_disk(PASSPHRASE).unwrap();
    store.store("acme", "jwt-1").unwrap();
    put_legacy(&store, "acme", "ancient-jwt");
    assert!(store.vault.contains("acme"));

    store.backend.fail_set = false;
    assert_eq!(
        store.store("acme", "jwt-2").unwrap(),
        StorageBackend::Keychain
    );
    assert!(
        !store.vault.exists(),
        "the last entry left the vault, so the file is gone"
    );
    assert!(store.load_legacy().is_empty());
    assert_eq!(store.get("acme").unwrap().as_deref(), Some("jwt-2"));
}

#[test]
fn reads_fall_back_when_the_keychain_errors_or_has_no_entry() {
    let (mut store, _dir) = store(failing());
    store.enable_disk(PASSPHRASE).unwrap();
    store.store("acme", "jwt-1").unwrap();

    store.backend.fail_set = false;
    store.backend.fail_get = true;
    assert_eq!(store.get("acme").unwrap().as_deref(), Some("jwt-1"));
    store.backend.fail_get = false;
    assert_eq!(
        store.get("acme").unwrap().as_deref(),
        Some("jwt-1"),
        "no keychain entry"
    );
    assert_eq!(store.get("missing").unwrap(), None);
}

#[test]
fn delete_removes_every_copy_and_reports_one_it_cannot_remove() {
    let keychain = FakeKeychain::default();
    keychain
        .secrets
        .borrow_mut()
        .insert("acme".into(), "kc".into());
    let (mut store, _dir) = store(keychain);
    store.enable_disk(PASSPHRASE).unwrap();
    store.vault.put(PASSPHRASE, "acme", "vault").unwrap();
    put_legacy(&store, "acme", "legacy");

    store.backend.fail_delete = true;
    let error = store.delete("acme").unwrap_err().to_string();
    assert!(error.contains("system keychain"), "{}", error);

    store.backend.fail_delete = false;
    store.delete("acme").unwrap();
    assert_eq!(store.get("acme").unwrap(), None);
    assert!(!store.has("acme"));
    store.delete("never-stored").unwrap();
}

#[test]
fn deleting_a_profile_removes_source_and_destination_jwt_and_password_and_keeps_others() {
    let (store, _dir) = store(FakeKeychain::default());
    for account in [
        "acme",
        "acme_password",
        "acme_dest",
        "acme_dest_password",
        "other",
    ] {
        store.store(account, "secret").unwrap();
    }
    store.delete_profile("acme").unwrap();
    for account in ["acme", "acme_password", "acme_dest", "acme_dest_password"] {
        assert_eq!(
            store.get(account).unwrap(),
            None,
            "{} should be gone",
            account
        );
    }
    assert!(store.get("other").unwrap().is_some());
}

#[test]
fn legacy_entries_move_into_the_keychain_and_the_old_file_is_deleted() {
    let (store, dir) = store(FakeKeychain::default());
    put_legacy(&store, "acme", "jwt-a");
    put_legacy(&store, "acme_password", "pw");

    let report = store.migrate_legacy().unwrap();
    assert_eq!(
        report,
        LegacyMigration {
            to_keychain: 2,
            to_encrypted_file: 0,
            remaining: 0
        }
    );
    assert_eq!(store.get("acme").unwrap().as_deref(), Some("jwt-a"));
    assert!(!dir.path().join("credentials.json").exists());
}

#[test]
fn without_a_keychain_legacy_entries_stay_readable_until_encrypted_storage_is_turned_on() {
    let (store, dir) = store(failing());
    put_legacy(&store, "acme", "jwt-a");

    let report = store.migrate_legacy().unwrap();
    assert_eq!(
        report,
        LegacyMigration {
            to_keychain: 0,
            to_encrypted_file: 0,
            remaining: 1
        }
    );
    assert_eq!(
        store.get("acme").unwrap().as_deref(),
        Some("jwt-a"),
        "nothing is lost"
    );
    assert_eq!(store.status().legacy_entries, 1);

    // Turning encrypted storage on re-encrypts them and removes the old file.
    let report = store.enable_disk(PASSPHRASE).unwrap();
    assert_eq!(
        report,
        LegacyMigration {
            to_keychain: 0,
            to_encrypted_file: 1,
            remaining: 0
        }
    );
    assert!(!dir.path().join("credentials.json").exists());
    assert_eq!(store.get("acme").unwrap().as_deref(), Some("jwt-a"));
    assert!(store.vault.contains("acme"));
    assert_eq!(store.status().legacy_entries, 0);
}

#[test]
fn status_reports_the_option_the_file_and_whether_it_is_unlocked() {
    let (store, _dir) = store(failing());
    assert_eq!(
        store.status(),
        StorageStatus {
            disk_enabled: false,
            vault_exists: false,
            unlocked: false,
            legacy_entries: 0
        }
    );
    store.enable_disk(PASSPHRASE).unwrap();
    store.store("acme", "jwt").unwrap();
    assert_eq!(
        store.status(),
        StorageStatus {
            disk_enabled: true,
            vault_exists: true,
            unlocked: true,
            legacy_entries: 0
        }
    );
    store.lock();
    assert!(!store.status().unlocked && store.status().disk_enabled);
}
