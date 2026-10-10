// Encrypted vault (`vault.enc`): Argon2id → AES-256-GCM; plaintext only while unlocked.

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
    // Never silently replace an existing vault with an empty one.
    if exists() {
        return Err(VaultError::Io("A vault already exists".into()));
    }
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

/// Whether a page origin may get autofill: HTTPS anywhere, plain HTTP only
/// for this machine and the local network (routers, NAS, dev servers), where
/// there is no CA to buy a certificate from. Public HTTP is MITM-able.
pub fn is_autofill_origin(origin: &str) -> bool {
    // Needs an explicit scheme: normalize_origin reads `data:...` as a bare
    // host and would call it https.
    if !origin.trim_start().contains("://") {
        return false;
    }
    let origin = normalize_origin(origin);
    let Some((scheme, hostport)) = origin.split_once("://") else {
        return false;
    };
    match scheme {
        "https" => true,
        "http" => is_local_host(host_of(hostport)),
        _ => false,
    }
}

/// Host part of `host[:port]`, `[v6]:port` or `user@host`.
fn host_of(hostport: &str) -> &str {
    let hostport = hostport.rsplit_once('@').map_or(hostport, |(_, h)| h);
    if let Some(rest) = hostport.strip_prefix('[') {
        return rest.split_once(']').map_or(rest, |(h, _)| h);
    }
    hostport.split_once(':').map_or(hostport, |(h, _)| h)
}

fn is_local_host(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => {
            // ::1, fc00::/7 unique-local, fe80::/10 link-local.
            ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
        Err(_) => false,
    }
}

/// Match key for autofill: hostname without leading `www.`, ignoring scheme.
/// So `https://www.example.com` and `http://example.com` share a vault entry.
pub fn host_key(url_or_origin: &str) -> String {
    let origin = normalize_origin(url_or_origin);
    let hostport = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin.as_str());
    hostport
        .strip_prefix("www.")
        .unwrap_or(hostport)
        .to_ascii_lowercase()
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
impl VaultSession {
    /// In-memory session for tests; never persisted.
    pub fn for_test(entries: Vec<VaultEntry>) -> Self {
        Self {
            key: KeyMaterial([0; KEY_LEN]),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_origin_strips_path_and_lowercases_host() {
        assert_eq!(normalize_origin("https://Example.COM/a?b#c"), "https://example.com");
        assert_eq!(normalize_origin("http://example.com:8080/x"), "http://example.com:8080");
        assert_eq!(normalize_origin("example.com/login"), "https://example.com");
        assert_eq!(normalize_origin("  https://a.test  "), "https://a.test");
    }

    #[test]
    fn host_key_ignores_scheme_and_www_only() {
        assert_eq!(host_key("https://www.example.com"), host_key("http://example.com"));
        assert_ne!(host_key("https://example.com:8443"), host_key("https://example.com"));
        assert_ne!(host_key("https://login.example.com"), host_key("https://example.com"));
    }

    #[test]
    fn autofill_origin_is_https_or_local_http() {
        for ok in [
            "https://bank.test",
            "https://bank.test:8443/login",
            "http://localhost:3000",
            "http://app.localhost",
            "http://router.local",
            "http://127.0.0.1:8080",
            "http://192.168.1.1",
            "http://10.0.0.5",
            "http://172.16.0.1",
            "http://169.254.1.1",
            "http://[::1]:8080",
            "http://[fd12::1]",
        ] {
            assert!(is_autofill_origin(ok), "{ok}");
        }
        for bad in [
            "http://bank.test",
            "http://8.8.8.8",
            "http://172.32.0.1",
            "http://localhost.evil.test",
            "http://192.168.1.1.evil.test",
            "http://[2001:db8::1]",
            "ftp://192.168.1.1",
            "data:text/html,x",
            "",
        ] {
            assert!(!is_autofill_origin(bad), "{bad}");
        }
    }

    #[test]
    fn host_key_does_not_strip_userinfo() {
        // `https://bank.test@evil.test/` must not match bank.test.
        assert_ne!(host_key("https://bank.test@evil.test/"), host_key("https://bank.test"));
    }

    #[test]
    fn entries_for_origin_matches_host_key() {
        let v = VaultSession::for_test(vec![
            test_entry("1", "https://www.a.test"),
            test_entry("2", "https://b.test"),
        ]);
        let ids: Vec<_> = v.entries_for_origin("http://a.test/login").iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["1"]);
    }

    #[test]
    fn generate_password_clamps_length_and_respects_symbols() {
        assert_eq!(generate_password(1, true).len(), 8);
        assert_eq!(generate_password(500, true).len(), 128);
        assert!(generate_password(64, false).chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
