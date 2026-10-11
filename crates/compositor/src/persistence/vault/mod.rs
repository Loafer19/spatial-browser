// Encrypted vault (`vault.enc`): Argon2id → AES-256-GCM; plaintext only while unlocked.
//
// File layout (all integers little-endian):
//   v1 (read-only, migrated on unlock):
//     "SBVT" | 0x01 | salt[16] | nonce[12] | ciphertext+tag
//     KDF = argon2 0.5 `Argon2::default()` (Argon2id, m=19 MiB, t=2, p=1); no AAD.
//   v2 (written):
//     "SBVT" | 0x02 | m_cost u32 (KiB) | t_cost u32 | p_cost u32 | salt[16] | nonce[12] | ciphertext+tag
//     KDF = Argon2id v0x13 with the stored params; AAD = every header byte before the ciphertext.
//
// Secrets in memory: the key, `VaultData` (every entry) and the plaintext JSON
// buffers are zeroized on drop. Limits: serde_json's parse scratch space,
// strings the CEF bridge decoded from `password://` URLs, and copies made by
// String reallocation are not scrubbed — there is no way to reach them
// without replacing serde/CEF plumbing.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::Rng;
use serde::{Deserialize, Serialize};
// rand 0.10: Rng::fill_bytes; avoid RngExt for broader compatibility.
use std::cell::Cell;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

mod format;
mod fs;
mod generator;
mod origin;
#[cfg(test)]
mod tests;

pub use format::KdfParams;
use format::*;
use fs::*;
pub use generator::*;
pub use origin::*;

/// Idle auto-lock: no vault use (list page action, fill, save, Ctrl+Shift+P)
/// for this long drops the key. A const until settings grow a field for it.
pub const AUTO_LOCK_AFTER: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Debug, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct VaultEntry {
    pub id: String,
    pub origin: String,
    pub username: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address_line1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct VaultData {
    pub version: u32,
    pub entries: Vec<VaultEntry>,
    #[serde(default)]
    pub never_save: Vec<String>,
}

/// Unlocked vault: key kept for re-encrypt on save; key and data zeroized on
/// drop, so locking is `Option::take`.
pub struct VaultSession {
    key: KeyMaterial,
    kdf: KdfParams,
    salt: [u8; SALT_LEN],
    path: PathBuf,
    last_used: Cell<Instant>,
    pub data: VaultData,
}

#[derive(Debug)]
pub enum VaultError {
    Io(String),
    Corrupt(&'static str),
    BadPassword,
    Serialize(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(s) => write!(f, "{s}"),
            Self::Corrupt(s) => write!(f, "corrupt vault: {s}"),
            Self::BadPassword => write!(f, "wrong master password"),
            Self::Serialize(s) => write!(f, "{s}"),
        }
    }
}

fn io_err(e: std::io::Error) -> VaultError {
    VaultError::Io(e.to_string())
}

pub fn path() -> PathBuf {
    crate::persistence::config_path("vault.enc")
}

pub fn exists() -> bool {
    path().is_file()
}

fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Create a new empty vault encrypted with `password`.
pub fn create(password: &str) -> Result<VaultSession, VaultError> {
    create_at(&path(), password, KdfParams::CURRENT)
}

fn create_at(path: &Path, password: &str, kdf: KdfParams) -> Result<VaultSession, VaultError> {
    // Never silently replace an existing vault with an empty one.
    if path.exists() {
        return Err(VaultError::Io("A vault already exists".into()));
    }
    let salt = random_salt();
    let key = derive_key(password, &salt, kdf)?;
    let session = VaultSession {
        key,
        kdf,
        salt,
        path: path.to_path_buf(),
        last_used: Cell::new(Instant::now()),
        data: VaultData {
            version: 1,
            entries: Vec::new(),
            never_save: Vec::new(),
        },
    };
    session.save()?;
    Ok(session)
}

impl VaultSession {
    pub fn unlock(password: &str) -> Result<Self, VaultError> {
        Self::unlock_at(&path(), password, KdfParams::CURRENT)
    }

    /// Decrypt `path`. A v1 file, or one with weaker KDF params than
    /// `target`, is re-encrypted with `target` (new salt + nonce) before
    /// returning; if that rewrite fails the old file stays and the session
    /// keeps working with the old key.
    fn unlock_at(path: &Path, password: &str, target: KdfParams) -> Result<Self, VaultError> {
        let bytes = std::fs::read(path).map_err(io_err)?;
        restrict_permissions(path);
        let header = parse_header(&bytes)?;
        let key = derive_key(password, &header.salt, header.kdf)?;
        let cipher =
            Aes256Gcm::new_from_slice(&key.0).map_err(|_| VaultError::Corrupt("bad key length"))?;
        let aad: &[u8] = if header.version == VERSION_V1 {
            &[]
        } else {
            &bytes[..header.len]
        };
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    Nonce::from_slice(&header.nonce),
                    Payload {
                        msg: &bytes[header.len..],
                        aad,
                    },
                )
                .map_err(|_| VaultError::BadPassword)?,
        );
        let data: VaultData =
            serde_json::from_slice(&plain).map_err(|e| VaultError::Serialize(e.to_string()))?;
        let mut session = Self {
            key,
            kdf: header.kdf,
            salt: header.salt,
            path: path.to_path_buf(),
            last_used: Cell::new(Instant::now()),
            data,
        };
        if header.version < VERSION_V2 || header.kdf.weaker_than(target) {
            match session.rekey(password, target) {
                Ok(()) => log::info!(
                    "vault: re-encrypted v{} file with Argon2id m={}KiB t={} p={}",
                    header.version,
                    target.m_cost,
                    target.t_cost,
                    target.p_cost
                ),
                Err(e) => log::warn!("vault: KDF upgrade failed, keeping old file: {e}"),
            }
        }
        Ok(session)
    }

    /// Re-encrypt under a fresh salt and `kdf`; the session switches keys
    /// only once the new file is on disk.
    fn rekey(&mut self, password: &str, kdf: KdfParams) -> Result<(), VaultError> {
        let salt = random_salt();
        let key = derive_key(password, &salt, kdf)?;
        let bytes = seal(&self.data, &key, kdf, &salt)?;
        write_atomic(&self.path, &bytes).map_err(io_err)?;
        self.key = key;
        self.kdf = kdf;
        self.salt = salt;
        Ok(())
    }

    /// Encrypt and atomically replace the vault file (same salt, new nonce).
    pub fn save(&self) -> Result<(), VaultError> {
        let bytes = seal(&self.data, &self.key, self.kdf, &self.salt)?;
        write_atomic(&self.path, &bytes).map_err(io_err)
    }

    /// Mark the vault as in use (resets the idle auto-lock timer).
    pub fn touch(&self) {
        self.last_used.set(Instant::now());
    }

    pub fn idle_for(&self) -> Duration {
        self.last_used.get().elapsed()
    }

    pub fn entries_for_origin(&self, origin: &str) -> Vec<&VaultEntry> {
        let key = host_key(origin);
        self.data
            .entries
            .iter()
            .filter(|e| host_key(&e.origin) == key)
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<&VaultEntry> {
        self.data.entries.iter().find(|e| e.id == id)
    }

    pub fn upsert(&mut self, mut entry: VaultEntry) -> Result<(), VaultError> {
        entry.updated_at = now_unix();
        if entry.id.is_empty() {
            entry.id = new_id();
        }
        if let Some(existing) = self.data.entries.iter_mut().find(|e| e.id == entry.id) {
            // Old value is dropped here, and VaultEntry scrubs on drop.
            *existing = entry;
        } else {
            self.data.entries.push(entry);
        }
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> Result<bool, VaultError> {
        let before = self.data.entries.len();
        self.data.entries.retain(|e| e.id != id);
        if self.data.entries.len() == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    pub fn is_never_save(&self, origin: &str) -> bool {
        let origin = normalize_origin(origin);
        self.data
            .never_save
            .iter()
            .any(|o| normalize_origin(o) == origin)
    }

    pub fn add_never_save(&mut self, origin: &str) -> Result<(), VaultError> {
        let origin = normalize_origin(origin);
        if !self.is_never_save(&origin) {
            self.data.never_save.push(origin);
            self.save()?;
        }
        Ok(())
    }

    pub fn remove_never_save(&mut self, origin: &str) -> Result<(), VaultError> {
        let origin = normalize_origin(origin);
        self.data
            .never_save
            .retain(|o| normalize_origin(o) != origin);
        self.save()
    }
}

#[cfg(test)]
impl VaultSession {
    /// In-memory session for tests; never persisted.
    pub fn for_test(entries: Vec<VaultEntry>) -> Self {
        Self {
            key: KeyMaterial([0; KEY_LEN]),
            kdf: KdfParams::CURRENT,
            salt: [0; SALT_LEN],
            path: PathBuf::new(),
            last_used: Cell::new(Instant::now()),
            data: VaultData {
                version: 1,
                entries,
                never_save: Vec::new(),
            },
        }
    }
}

#[cfg(test)]
pub fn test_entry(id: &str, origin: &str) -> VaultEntry {
    VaultEntry {
        id: id.into(),
        origin: origin.into(),
        username: "user".into(),
        password: "pw".into(),
        email: None,
        address_line1: None,
        city: None,
        postal_code: None,
        country: None,
        notes: None,
        updated_at: 0,
    }
}
