//! Passphrase-encrypted credential file, used only when the user opts in to storing
//! credentials on disk because no OS keychain is available.
//!
//! - The key is derived from the user's passphrase with Argon2id and a random per-file salt,
//!   so the file is only as readable as the passphrase is guessable. There is no key stored
//!   alongside it.
//! - Each secret is sealed with ChaCha20-Poly1305 under a fresh random nonce. The account name
//!   is bound in as associated data, so an entry cannot be moved to another account.
//! - A sealed "check" value lets a wrong passphrase be told apart from a corrupted file before
//!   anything is written.
//! - The file is written atomically and, on Unix, readable only by the current user.
//!
//! Account names are stored in the clear (they are profile names); only secrets are sealed.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// Shortest passphrase accepted when a vault is created.
pub const MIN_PASSPHRASE_LEN: usize = 12;

const FORMAT_VERSION: u32 = 1;
const CHECK_PLAINTEXT: &[u8] = b"nxport-vault-check";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Argon2id cost settings (OWASP's minimum recommendation for Argon2id).
#[derive(Debug, Clone, Copy)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            memory_kib: 19 * 1024,
            iterations: 2,
            parallelism: 1,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum VaultError {
    /// The passphrase does not open this vault.
    WrongPassphrase,
    /// A new vault needs a passphrase of at least [`MIN_PASSPHRASE_LEN`] characters.
    PassphraseTooShort,
    /// The file is not a vault this version understands.
    Unsupported(String),
    /// The file is damaged or has been tampered with.
    Corrupt(String),
    Io(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::WrongPassphrase => write!(f, "wrong passphrase for the credentials file"),
            VaultError::PassphraseTooShort => write!(
                f,
                "the passphrase must be at least {} characters",
                MIN_PASSPHRASE_LEN
            ),
            VaultError::Unsupported(why) => write!(f, "unsupported credentials file: {}", why),
            VaultError::Corrupt(why) => write!(f, "credentials file is damaged: {}", why),
            VaultError::Io(why) => write!(f, "credentials file error: {}", why),
        }
    }
}

impl std::error::Error for VaultError {}

type Result<T> = std::result::Result<T, VaultError>;

#[derive(Serialize, Deserialize)]
struct KdfRecord {
    alg: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Sealed {
    nonce: String,
    ciphertext: String,
}

#[derive(Serialize, Deserialize)]
struct VaultFile {
    version: u32,
    kdf: KdfRecord,
    check: Sealed,
    entries: BTreeMap<String, Sealed>,
}

pub struct Vault {
    path: PathBuf,
    params: KdfParams,
}

fn aad(account: &str) -> Vec<u8> {
    format!("nxport-vault-v{}:{}", FORMAT_VERSION, account).into_bytes()
}

fn derive_key(passphrase: &str, record: &KdfRecord) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    if record.alg != "argon2id" {
        return Err(VaultError::Unsupported(format!(
            "key derivation '{}'",
            record.alg
        )));
    }
    let salt = BASE64
        .decode(&record.salt)
        .map_err(|_| VaultError::Corrupt("salt is not valid".into()))?;
    let params = argon2::Params::new(
        record.memory_kib,
        record.iterations,
        record.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|e| VaultError::Corrupt(format!("key derivation settings: {}", e)))?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(passphrase.as_bytes(), &salt, key.as_mut())
        .map_err(|e| VaultError::Corrupt(format!("key derivation failed: {}", e)))?;
    Ok(key)
}

fn seal(key: &[u8; KEY_LEN], plaintext: &[u8], aad: &[u8]) -> Result<Sealed> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| VaultError::Corrupt("encryption failed".into()))?;
    Ok(Sealed {
        nonce: BASE64.encode(nonce),
        ciphertext: BASE64.encode(ciphertext),
    })
}

/// `None` means the authentication tag did not verify: wrong key, or altered data.
fn open(key: &[u8; KEY_LEN], sealed: &Sealed, aad: &[u8]) -> Result<Option<Zeroizing<Vec<u8>>>> {
    let nonce = BASE64
        .decode(&sealed.nonce)
        .ok()
        .filter(|nonce| nonce.len() == NONCE_LEN)
        .ok_or_else(|| VaultError::Corrupt("nonce is not valid".into()))?;
    let ciphertext = BASE64
        .decode(&sealed.ciphertext)
        .map_err(|_| VaultError::Corrupt("ciphertext is not valid".into()))?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    Ok(cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad,
            },
        )
        .ok()
        .map(Zeroizing::new))
}

impl Vault {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            params: KdfParams::default(),
        }
    }

    /// A vault with cheaper key derivation, for tests.
    #[cfg(test)]
    pub fn with_params(path: PathBuf, params: KdfParams) -> Self {
        Self { path, params }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    fn read(&self) -> Result<Option<VaultFile>> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(VaultError::Io(e.to_string())),
        };
        let file: VaultFile =
            serde_json::from_str(&text).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        if file.version != FORMAT_VERSION {
            return Err(VaultError::Unsupported(format!(
                "format version {}",
                file.version
            )));
        }
        Ok(Some(file))
    }

    fn write(&self, file: &VaultFile) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| VaultError::Io(e.to_string()))?;
        }
        let json = serde_json::to_string_pretty(file).map_err(|e| VaultError::Io(e.to_string()))?;
        // Write beside the target and rename, so a crash never leaves a half-written vault.
        let temp = self.path.with_extension("tmp");
        write_private(&temp, &json).map_err(|e| VaultError::Io(e.to_string()))?;
        fs::rename(&temp, &self.path).map_err(|e| VaultError::Io(e.to_string()))
    }

    /// Derive the key for an existing vault and confirm the passphrase opens it.
    fn unlock(&self, file: &VaultFile, passphrase: &str) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        let key = derive_key(passphrase, &file.kdf)?;
        match open(&key, &file.check, &aad("check"))? {
            Some(plain) if plain.as_slice() == CHECK_PLAINTEXT => Ok(key),
            _ => Err(VaultError::WrongPassphrase),
        }
    }

    /// Confirm `passphrase` opens the vault. Succeeds if there is no vault yet.
    pub fn verify(&self, passphrase: &str) -> Result<()> {
        match self.read()? {
            Some(file) => self.unlock(&file, passphrase).map(|_| ()),
            None => Ok(()),
        }
    }

    /// Store a secret, creating the vault if needed. A new vault needs a long-enough
    /// passphrase; an existing one must be opened with its own passphrase.
    pub fn put(&self, passphrase: &str, account: &str, secret: &str) -> Result<()> {
        let (mut file, key) = match self.read()? {
            Some(file) => {
                let key = self.unlock(&file, passphrase)?;
                (file, key)
            }
            None => {
                if passphrase.chars().count() < MIN_PASSPHRASE_LEN {
                    return Err(VaultError::PassphraseTooShort);
                }
                let mut salt = [0u8; SALT_LEN];
                OsRng.fill_bytes(&mut salt);
                let record = KdfRecord {
                    alg: "argon2id".into(),
                    memory_kib: self.params.memory_kib,
                    iterations: self.params.iterations,
                    parallelism: self.params.parallelism,
                    salt: BASE64.encode(salt),
                };
                let key = derive_key(passphrase, &record)?;
                let check = seal(&key, CHECK_PLAINTEXT, &aad("check"))?;
                (
                    VaultFile {
                        version: FORMAT_VERSION,
                        kdf: record,
                        check,
                        entries: BTreeMap::new(),
                    },
                    key,
                )
            }
        };
        file.entries.insert(
            account.to_string(),
            seal(&key, secret.as_bytes(), &aad(account))?,
        );
        self.write(&file)
    }

    pub fn get(&self, passphrase: &str, account: &str) -> Result<Option<Zeroizing<String>>> {
        let Some(file) = self.read()? else {
            return Ok(None);
        };
        let key = self.unlock(&file, passphrase)?;
        let Some(sealed) = file.entries.get(account) else {
            return Ok(None);
        };
        match open(&key, sealed, &aad(account))? {
            Some(plain) => String::from_utf8(plain.to_vec())
                .map(|text| Some(Zeroizing::new(text)))
                .map_err(|_| VaultError::Corrupt(format!("entry '{}' is not text", account))),
            None => Err(VaultError::Corrupt(format!(
                "entry '{}' failed its integrity check",
                account
            ))),
        }
    }

    /// Whether an entry exists. Needs no passphrase: account names are not secret.
    pub fn contains(&self, account: &str) -> bool {
        self.read()
            .ok()
            .flatten()
            .is_some_and(|file| file.entries.contains_key(account))
    }

    /// Remove an entry. Needs no passphrase, so a locked vault can still be cleaned up.
    /// An empty vault is deleted, so no file remains once nothing is stored.
    pub fn remove(&self, account: &str) -> Result<()> {
        let Some(mut file) = self.read()? else {
            return Ok(());
        };
        if file.entries.remove(account).is_none() {
            return Ok(());
        }
        if file.entries.is_empty() {
            return self.delete();
        }
        self.write(&file)
    }

    /// Delete the whole vault. This is how a forgotten passphrase is reset: the stored
    /// credentials are lost and must be entered again.
    pub fn delete(&self) -> Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(VaultError::Io(e.to_string())),
        }
    }
}

/// Write a file readable only by the current user (where the platform supports it).
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        file.write_all(contents.as_bytes())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests;
