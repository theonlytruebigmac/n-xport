use super::*;
use tempfile::TempDir;

const PASSPHRASE: &str = "correct horse battery";

/// Cheap key derivation keeps the tests fast; the settings are stored in the file.
fn vault() -> (Vault, TempDir) {
    let dir = TempDir::new().expect("temp dir");
    let vault = Vault::with_params(
        dir.path().join("vault.json"),
        KdfParams {
            memory_kib: 8,
            iterations: 1,
            parallelism: 1,
        },
    );
    (vault, dir)
}

#[test]
fn a_secret_round_trips_and_is_not_in_the_file_in_any_readable_form() {
    let (vault, _dir) = vault();
    vault.put(PASSPHRASE, "acme", "super-secret-jwt").unwrap();

    let got = vault.get(PASSPHRASE, "acme").unwrap().unwrap();
    assert_eq!(got.as_str(), "super-secret-jwt");

    let on_disk = fs::read_to_string(vault.path()).unwrap();
    assert!(!on_disk.contains("super-secret-jwt"));
    assert!(!on_disk.contains(&BASE64.encode("super-secret-jwt")));
    assert!(on_disk.contains("argon2id") && on_disk.contains("acme"));
}

#[test]
fn the_wrong_passphrase_opens_nothing_and_writes_nothing() {
    let (vault, _dir) = vault();
    vault.put(PASSPHRASE, "acme", "jwt-1").unwrap();
    let before = fs::read_to_string(vault.path()).unwrap();

    assert_eq!(
        vault.get("a different passphrase", "acme").unwrap_err(),
        VaultError::WrongPassphrase
    );
    assert_eq!(
        vault.verify("a different passphrase").unwrap_err(),
        VaultError::WrongPassphrase
    );
    assert_eq!(
        vault
            .put("a different passphrase", "acme", "jwt-2")
            .unwrap_err(),
        VaultError::WrongPassphrase
    );
    assert_eq!(
        fs::read_to_string(vault.path()).unwrap(),
        before,
        "a failed unlock must not modify the vault"
    );
    assert_eq!(
        vault.get(PASSPHRASE, "acme").unwrap().unwrap().as_str(),
        "jwt-1"
    );
}

#[test]
fn a_new_vault_refuses_a_short_passphrase_but_an_existing_one_is_unaffected() {
    let (vault, _dir) = vault();
    assert_eq!(
        vault.put("short", "acme", "jwt").unwrap_err(),
        VaultError::PassphraseTooShort
    );
    assert!(
        !vault.exists(),
        "nothing is created for a rejected passphrase"
    );
    assert_eq!(vault.put("exactly12chr", "acme", "jwt").map(|_| ()), Ok(()));
}

#[test]
fn each_save_uses_a_fresh_nonce_and_the_same_secret_seals_differently() {
    let (vault, _dir) = vault();
    vault.put(PASSPHRASE, "a", "same-secret").unwrap();
    vault.put(PASSPHRASE, "b", "same-secret").unwrap();
    let file = vault.read().unwrap().unwrap();
    assert_ne!(file.entries["a"].nonce, file.entries["b"].nonce);
    assert_ne!(file.entries["a"].ciphertext, file.entries["b"].ciphertext);
}

#[test]
fn an_entry_moved_to_another_account_fails_its_integrity_check() {
    let (vault, _dir) = vault();
    vault.put(PASSPHRASE, "low-privilege", "token-a").unwrap();
    vault.put(PASSPHRASE, "admin", "token-b").unwrap();
    let mut file = vault.read().unwrap().unwrap();
    let swapped = file.entries["low-privilege"].clone();
    file.entries.insert("admin".into(), swapped);
    vault.write(&file).unwrap();

    assert!(matches!(
        vault.get(PASSPHRASE, "admin"),
        Err(VaultError::Corrupt(_))
    ));
}

#[test]
fn tampering_with_a_ciphertext_is_detected() {
    let (vault, _dir) = vault();
    vault.put(PASSPHRASE, "acme", "jwt").unwrap();
    let mut file = vault.read().unwrap().unwrap();
    let sealed = file.entries.get_mut("acme").unwrap();
    let mut bytes = BASE64.decode(&sealed.ciphertext).unwrap();
    bytes[0] ^= 0x01;
    sealed.ciphertext = BASE64.encode(bytes);
    vault.write(&file).unwrap();

    assert!(matches!(
        vault.get(PASSPHRASE, "acme"),
        Err(VaultError::Corrupt(_))
    ));
}

#[test]
fn a_vault_opens_with_the_settings_stored_in_it_not_the_current_defaults() {
    let (vault, dir) = vault();
    vault.put(PASSPHRASE, "acme", "jwt").unwrap();
    // A newer build would use different default costs; the file must still open.
    let newer = Vault::new(dir.path().join("vault.json"));
    assert_eq!(
        newer.get(PASSPHRASE, "acme").unwrap().unwrap().as_str(),
        "jwt"
    );
}

#[test]
fn contains_and_remove_need_no_passphrase_and_an_empty_vault_leaves_no_file() {
    let (vault, _dir) = vault();
    vault.put(PASSPHRASE, "a", "1").unwrap();
    vault.put(PASSPHRASE, "b", "2").unwrap();
    assert!(vault.contains("a") && vault.contains("b") && !vault.contains("c"));

    vault.remove("a").unwrap();
    assert!(!vault.contains("a") && vault.contains("b"));
    vault.remove("never-stored").unwrap();
    vault.remove("b").unwrap();
    assert!(
        !vault.exists(),
        "no file remains once the last entry is removed"
    );
}

#[test]
fn unreadable_or_unsupported_files_are_reported_not_overwritten() {
    let (vault, _dir) = vault();
    fs::write(vault.path(), "this is not json").unwrap();
    assert!(matches!(
        vault.get(PASSPHRASE, "a"),
        Err(VaultError::Corrupt(_))
    ));
    assert!(matches!(
        vault.put(PASSPHRASE, "a", "x"),
        Err(VaultError::Corrupt(_))
    ));
    assert_eq!(
        fs::read_to_string(vault.path()).unwrap(),
        "this is not json"
    );

    fs::write(vault.path(), r#"{"version":99,"kdf":{"alg":"argon2id","memory_kib":8,"iterations":1,"parallelism":1,"salt":""},"check":{"nonce":"","ciphertext":""},"entries":{}}"#).unwrap();
    assert!(matches!(
        vault.get(PASSPHRASE, "a"),
        Err(VaultError::Unsupported(_))
    ));
}

#[test]
fn a_missing_vault_has_nothing_to_read_and_verifies_any_passphrase() {
    let (vault, _dir) = vault();
    assert_eq!(vault.get(PASSPHRASE, "acme").unwrap(), None);
    assert_eq!(vault.verify("anything"), Ok(()));
    assert!(!vault.contains("acme"));
}

#[cfg(unix)]
#[test]
fn the_vault_file_is_private_to_the_user_and_no_temporary_file_is_left() {
    use std::os::unix::fs::PermissionsExt;
    let (vault, dir) = vault();
    vault.put(PASSPHRASE, "acme", "jwt").unwrap();
    let mode = fs::metadata(vault.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}
