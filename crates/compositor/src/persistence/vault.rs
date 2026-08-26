// Encrypted local password vault (`~/.config/spatial-browser/vault.enc`).
// Argon2id → AES-256-GCM. Unlocked plaintext lives in memory only while
// the process holds a VaultSession (until exit — see passwords.rs).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use rand::Rng;
use serde::{Deserialize, Serialize};
// rand 0.10: Rng::fill_bytes; avoid RngExt for broader compatibility.
use std::path::PathBuf;
use zeroize::{Zeroize, ZeroizeOnDrop};

const MAGIC: &[u8; 4] = b"SBVT";
const FORMAT_VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultEntry {
    pub id: String,
    pub origin: String,
    pub username: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VaultData {
    pub version: u32,
    pub entries: Vec<VaultEntry>,
    #[serde(default)]
    pub never_save: Vec<String>,
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct KeyMaterial([u8; KEY_LEN]);

/// Unlocked vault: key kept for re-encrypt on save; zeroized on drop.
pub struct VaultSession {
    key: KeyMaterial,
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

pub fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/vault.enc")
}

pub fn exists() -> bool {
    path().is_file()
}

fn derive_key(password: &str, salt: &[u8; SALT_LEN]) -> Result<KeyMaterial, VaultError> {
    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| VaultError::Corrupt("argon2 failed"))?;
    Ok(KeyMaterial(key))
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
    let mut salt = [0u8; SALT_LEN];
    rand::rng().fill_bytes(&mut salt);
    let key = derive_key(password, &salt)?;
    let data = VaultData {
        version: 1,
        entries: Vec::new(),
        never_save: Vec::new(),
    };
    let session = VaultSession { key, data };
    session.persist_with_salt(&salt)?;
    Ok(session)
}

impl VaultSession {
    pub fn unlock(password: &str) -> Result<Self, VaultError> {
        let bytes = std::fs::read(path()).map_err(|e| VaultError::Io(e.to_string()))?;
        if bytes.len() < 4 + 1 + SALT_LEN + NONCE_LEN + 16 {
            return Err(VaultError::Corrupt("too short"));
        }
        if &bytes[0..4] != MAGIC {
            return Err(VaultError::Corrupt("bad magic"));
        }
        if bytes[4] != FORMAT_VERSION {
            return Err(VaultError::Corrupt("unsupported version"));
        }
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&bytes[5..5 + SALT_LEN]);
        let nonce_start = 5 + SALT_LEN;
        let nonce = Nonce::from_slice(&bytes[nonce_start..nonce_start + NONCE_LEN]);
        let ciphertext = &bytes[nonce_start + NONCE_LEN..];

        let key = derive_key(password, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&key.0)
            .map_err(|_| VaultError::Corrupt("bad key length"))?;
        let plain = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| VaultError::BadPassword)?;
        let data: VaultData =
            serde_json::from_slice(&plain).map_err(|e| VaultError::Serialize(e.to_string()))?;
        Ok(Self { key, data })
    }

    fn persist_with_salt(&self, salt: &[u8; SALT_LEN]) -> Result<(), VaultError> {
        let plain = serde_json::to_vec(&self.data)
            .map_err(|e| VaultError::Serialize(e.to_string()))?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let cipher = Aes256Gcm::new_from_slice(&self.key.0)
            .map_err(|_| VaultError::Corrupt("bad key length"))?;
        let ciphertext = cipher
            .encrypt(nonce, plain.as_ref())
            .map_err(|_| VaultError::Corrupt("encrypt failed"))?;

        let mut out = Vec::with_capacity(4 + 1 + SALT_LEN + NONCE_LEN + ciphertext.len());
        out.extend_from_slice(MAGIC);
        out.push(FORMAT_VERSION);
        out.extend_from_slice(salt);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);

        let path = path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| VaultError::Io(e.to_string()))?;
        }
        let tmp = path.with_extension("enc.tmp");
        std::fs::write(&tmp, &out).map_err(|e| VaultError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &path).map_err(|e| VaultError::Io(e.to_string()))?;
        Ok(())
    }

    /// Re-read salt from disk and persist (same salt, new nonce).
    pub fn save(&self) -> Result<(), VaultError> {
        let bytes = std::fs::read(path()).map_err(|e| VaultError::Io(e.to_string()))?;
        if bytes.len() < 5 + SALT_LEN {
            return Err(VaultError::Corrupt("missing salt"));
        }
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&bytes[5..5 + SALT_LEN]);
        self.persist_with_salt(&salt)
    }

    pub fn entries_for_origin(&self, origin: &str) -> Vec<&VaultEntry> {
        let origin = normalize_origin(origin);
        self.data
            .entries
            .iter()
            .filter(|e| normalize_origin(&e.origin) == origin)
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
        self.data.never_save.retain(|o| normalize_origin(o) != origin);
        self.save()
    }
}

/// Strip path/query; keep scheme + host (+ non-default port).
pub fn normalize_origin(url_or_origin: &str) -> String {
    let s = url_or_origin.trim();
    let rest = if let Some(r) = s.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = s.strip_prefix("http://") {
        ("http", r)
    } else if s.contains("://") {
        return s
            .split_once("://")
            .map(|(sch, r)| {
                let hostport = r.split(['/', '?', '#']).next().unwrap_or(r);
                format!("{sch}://{}", hostport.to_ascii_lowercase())
            })
            .unwrap_or_else(|| s.to_string());
    } else {
        return format!("https://{}", s.split(['/', '?', '#']).next().unwrap_or(s).to_ascii_lowercase());
    };
    let (scheme, after) = rest;
    let hostport = after.split(['/', '?', '#']).next().unwrap_or(after);
    format!("{scheme}://{}", hostport.to_ascii_lowercase())
}

pub fn generate_password(length: usize, symbols: bool) -> String {
    const ALPHA: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    const SYM: &[u8] = b"!@#$%^&*-_=+?";
    let alphabet: Vec<u8> = if symbols {
        ALPHA.iter().chain(SYM.iter()).copied().collect()
    } else {
        ALPHA.to_vec()
    };
    let len = length.clamp(8, 128);
    let mut rng = rand::rng();
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        let mut b = [0u8; 1];
        rng.fill_bytes(&mut b);
        out.push(alphabet[b[0] as usize % alphabet.len()] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

    #[test]
    fn round_trip_in_memory_style() {
        let mut salt = [0u8; SALT_LEN];
        rand::rng().fill_bytes(&mut salt);
        let key = derive_key("test-pass", &salt).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key.0).unwrap();
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher.encrypt(nonce, b"payload".as_ref()).unwrap();
        let pt = cipher.decrypt(nonce, ct.as_ref()).unwrap();
        assert_eq!(pt, b"payload");
    }

    #[test]
    fn origin_normalize() {
        assert_eq!(
            normalize_origin("https://Example.com/login?x=1"),
            "https://example.com"
        );
    }
}
