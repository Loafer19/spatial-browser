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

const MAGIC: &[u8; 4] = b"SBVT";
const VERSION_V1: u8 = 1;
const VERSION_V2: u8 = 2;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
const V1_HEADER_LEN: usize = 4 + 1 + SALT_LEN + NONCE_LEN;
const V2_HEADER_LEN: usize = 4 + 1 + 3 * 4 + SALT_LEN + NONCE_LEN;

/// Idle auto-lock: no vault use (list page action, fill, save, Ctrl+Shift+P)
/// for this long drops the key. A const until settings grow a field for it.
pub const AUTO_LOCK_AFTER: Duration = Duration::from_secs(15 * 60);

/// Argon2id cost parameters, stored in the v2 header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    /// New vaults and migrations: OWASP's Argon2id tier (64 MiB, 3 passes).
    pub const CURRENT: Self = Self {
        m_cost: 64 * 1024,
        t_cost: 3,
        p_cost: 1,
    };

    /// What v1 files were encrypted with: argon2 0.5 `Argon2::default()`.
    const V1: Self = Self {
        m_cost: Params::DEFAULT_M_COST,
        t_cost: Params::DEFAULT_T_COST,
        p_cost: Params::DEFAULT_P_COST,
    };

    /// Bounds for header values: a corrupt or tampered file must not make
    /// unlock allocate gigabytes or spin for minutes.
    fn validate(self) -> Result<Self, VaultError> {
        let ok = (1..=16).contains(&self.p_cost)
            && (1..=32).contains(&self.t_cost)
            && self.m_cost >= 8 * self.p_cost
            && self.m_cost <= 1024 * 1024;
        if ok {
            Ok(self)
        } else {
            Err(VaultError::Corrupt("KDF parameters out of range"))
        }
    }

    fn weaker_than(self, other: Self) -> bool {
        self.m_cost < other.m_cost || self.t_cost < other.t_cost
    }
}

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

#[derive(Zeroize, ZeroizeOnDrop)]
struct KeyMaterial([u8; KEY_LEN]);

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
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/vault.enc")
}

pub fn exists() -> bool {
    path().is_file()
}

fn derive_key(password: &str, salt: &[u8; SALT_LEN], kdf: KdfParams) -> Result<KeyMaterial, VaultError> {
    let params = Params::new(kdf.m_cost, kdf.t_cost, kdf.p_cost, Some(KEY_LEN))
        .map_err(|_| VaultError::Corrupt("bad KDF parameters"))?;
    // Hash straight into the zeroize-on-drop buffer; no stray stack copy.
    let mut key = KeyMaterial([0u8; KEY_LEN]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut key.0)
        .map_err(|_| VaultError::Corrupt("argon2 failed"))?;
    Ok(key)
}

fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    rand::rng().fill_bytes(&mut salt);
    salt
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

struct Header {
    version: u8,
    kdf: KdfParams,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    /// Bytes before the ciphertext (the AAD for v2).
    len: usize,
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&bytes[at..at + 4]);
    u32::from_le_bytes(b)
}

fn parse_header(bytes: &[u8]) -> Result<Header, VaultError> {
    if bytes.len() < 5 {
        return Err(VaultError::Corrupt("too short"));
    }
    if &bytes[0..4] != MAGIC {
        return Err(VaultError::Corrupt("bad magic"));
    }
    let version = bytes[4];
    let (kdf, salt_at, len) = match version {
        VERSION_V1 => (KdfParams::V1, 5, V1_HEADER_LEN),
        VERSION_V2 => {
            if bytes.len() < V2_HEADER_LEN {
                return Err(VaultError::Corrupt("too short"));
            }
            let kdf = KdfParams {
                m_cost: read_u32(bytes, 5),
                t_cost: read_u32(bytes, 9),
                p_cost: read_u32(bytes, 13),
            }
            .validate()?;
            (kdf, 17, V2_HEADER_LEN)
        }
        _ => return Err(VaultError::Corrupt("unsupported version")),
    };
    if bytes.len() < len + TAG_LEN {
        return Err(VaultError::Corrupt("too short"));
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&bytes[salt_at..salt_at + SALT_LEN]);
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&bytes[salt_at + SALT_LEN..len]);
    Ok(Header {
        version,
        kdf,
        salt,
        nonce,
        len,
    })
}

/// Serialize into an exactly-sized buffer: letting serde_json grow a Vec
/// would free partial plaintext copies without scrubbing them.
fn to_json(data: &VaultData) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let ser = |e: serde_json::Error| VaultError::Serialize(e.to_string());
    let mut count = Count(0);
    serde_json::to_writer(&mut count, data).map_err(ser)?;
    let mut buf = Zeroizing::new(Vec::with_capacity(count.0));
    serde_json::to_writer(&mut *buf, data).map_err(ser)?;
    Ok(buf)
}

/// Encrypt `data` into a complete v2 file image (fresh nonce).
fn seal(
    data: &VaultData,
    key: &KeyMaterial,
    kdf: KdfParams,
    salt: &[u8; SALT_LEN],
) -> Result<Vec<u8>, VaultError> {
    let plain = to_json(data)?;
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);

    let mut out = Vec::with_capacity(V2_HEADER_LEN + plain.len() + TAG_LEN);
    out.extend_from_slice(MAGIC);
    out.push(VERSION_V2);
    out.extend_from_slice(&kdf.m_cost.to_le_bytes());
    out.extend_from_slice(&kdf.t_cost.to_le_bytes());
    out.extend_from_slice(&kdf.p_cost.to_le_bytes());
    out.extend_from_slice(salt);
    out.extend_from_slice(&nonce);

    let cipher =
        Aes256Gcm::new_from_slice(&key.0).map_err(|_| VaultError::Corrupt("bad key length"))?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: &out,
            },
        )
        .map_err(|_| VaultError::Corrupt("encrypt failed"))?;
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Replace `path` with `bytes` so a crash leaves either the old or the new
/// file: write a 0600 temp file, fsync, rename over, fsync the directory.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir)?;
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    // A leftover temp file from a crash may carry looser permissions;
    // `create_new` below guarantees ours is fresh with mode 0600.
    match std::fs::remove_file(&tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        std::fs::File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
        let cipher = Aes256Gcm::new_from_slice(&key.0)
            .map_err(|_| VaultError::Corrupt("bad key length"))?;
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
        self.data.never_save.retain(|o| normalize_origin(o) != origin);
        self.save()
    }
}

/// Vaults written before files were created 0600 inherited the umask
/// (usually 0644); tighten on read. Best effort — unlock still works.
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.permissions().mode() & 0o077 != 0 {
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            log::warn!("vault: chmod 600 {}: {e}", path.display());
        }
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

const GEN_ALPHA: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const GEN_SYM: &[u8] = b"!@#$%^&*-_=+?";

pub fn generate_password(length: usize, symbols: bool) -> String {
    let alphabet: Vec<u8> = if symbols {
        GEN_ALPHA.iter().chain(GEN_SYM.iter()).copied().collect()
    } else {
        GEN_ALPHA.to_vec()
    };
    let len = length.clamp(8, 128);
    let mut rng = rand::rng();
    let mut next_byte = || {
        let mut b = [0u8; 1];
        rng.fill_bytes(&mut b);
        b[0]
    };
    (0..len)
        .map(|_| alphabet[uniform_index(alphabet.len(), &mut next_byte)] as char)
        .collect()
}

/// Uniform index in `0..n` (1 ≤ n ≤ 256) from random bytes. Rejection
/// sampling: bytes at or above the largest multiple of `n` are redrawn, so
/// every index is equally likely (plain `b % n` favours low indices).
fn uniform_index(n: usize, next_byte: &mut impl FnMut() -> u8) -> usize {
    assert!((1..=256).contains(&n), "alphabet size {n} out of range");
    let zone = 256 - 256 % n;
    loop {
        let b = next_byte() as usize;
        if b < zone {
            return b % n;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Cheap params so tests don't spend seconds in Argon2.
    const FAST: KdfParams = KdfParams {
        m_cost: 64,
        t_cost: 1,
        p_cost: 1,
    };

    /// Per-test temp directory (removed on drop). Tests run in parallel,
    /// so they never touch HOME or the real vault path.
    struct TempVault(PathBuf);

    impl TempVault {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("sb-vault-test-{name}-{}", new_id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> PathBuf {
            self.0.join("vault.enc")
        }
    }

    impl Drop for TempVault {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// v1 file exactly as the pre-v2 code wrote it.
    fn write_v1(path: &Path, password: &str, data: &VaultData) {
        let mut salt = [0u8; SALT_LEN];
        rand::rng().fill_bytes(&mut salt);
        let mut key = [0u8; KEY_LEN];
        Argon2::default()
            .hash_password_into(password.as_bytes(), &salt, &mut key)
            .unwrap();
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);
        let ct = Aes256Gcm::new_from_slice(&key)
            .unwrap()
            .encrypt(Nonce::from_slice(&nonce), serde_json::to_vec(data).unwrap().as_ref())
            .unwrap();
        let mut out = MAGIC.to_vec();
        out.push(VERSION_V1);
        out.extend_from_slice(&salt);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn create_then_unlock_round_trips() {
        let tmp = TempVault::new("roundtrip");
        let path = tmp.path();
        let mut v = create_at(&path, "correct horse", FAST).unwrap();
        v.upsert(test_entry("", "https://a.test")).unwrap();
        drop(v);

        let bytes = std::fs::read(&path).unwrap();
        let header = parse_header(&bytes).unwrap();
        assert_eq!(header.version, VERSION_V2);
        assert_eq!(header.kdf, FAST);

        let v = VaultSession::unlock_at(&path, "correct horse", FAST).unwrap();
        assert_eq!(v.data.entries.len(), 1);
        assert_eq!(v.data.entries[0].password, "pw");
        // Same params → no rewrite, salt unchanged.
        assert_eq!(v.salt, header.salt);
    }

    #[test]
    fn create_refuses_to_overwrite() {
        let tmp = TempVault::new("exists");
        let path = tmp.path();
        create_at(&path, "password1", FAST).unwrap();
        assert!(create_at(&path, "password2", FAST).is_err());
        assert!(VaultSession::unlock_at(&path, "password1", FAST).is_ok());
    }

    #[test]
    fn wrong_password_fails() {
        let tmp = TempVault::new("wrong");
        let path = tmp.path();
        create_at(&path, "password1", FAST).unwrap();
        assert!(matches!(
            VaultSession::unlock_at(&path, "password2", FAST),
            Err(VaultError::BadPassword)
        ));
    }

    #[test]
    fn tampered_header_fails_auth() {
        let tmp = TempVault::new("tamper");
        let path = tmp.path();
        create_at(&path, "password1", FAST).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        // Bump t_cost: still in range, so this reaches AES-GCM, where the
        // header-as-AAD must reject it.
        bytes[9] = 2;
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            VaultSession::unlock_at(&path, "password1", FAST),
            Err(VaultError::BadPassword)
        ));
    }

    #[test]
    fn out_of_range_params_rejected_before_kdf() {
        let tmp = TempVault::new("dos");
        let path = tmp.path();
        create_at(&path, "password1", FAST).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            VaultSession::unlock_at(&path, "password1", FAST),
            Err(VaultError::Corrupt(_))
        ));
    }

    #[test]
    fn v1_vault_unlocks_and_migrates() {
        let tmp = TempVault::new("v1");
        let path = tmp.path();
        let data = VaultData {
            version: 1,
            entries: vec![test_entry("x", "https://old.test")],
            never_save: vec!["https://never.test".into()],
        };
        write_v1(&path, "old master", &data);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let old_salt = parse_header(&std::fs::read(&path).unwrap()).unwrap().salt;

        let mut v = VaultSession::unlock_at(&path, "old master", FAST).unwrap();
        assert_eq!(v.data.entries[0].origin, "https://old.test");
        assert_eq!(v.data.never_save, ["https://never.test"]);

        let header = parse_header(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(header.version, VERSION_V2);
        assert_eq!(header.kdf, FAST);
        assert_ne!(header.salt, old_salt);
        assert_eq!(mode(&path), 0o600);

        // The session uses the new key for later saves.
        v.upsert(test_entry("", "https://new.test")).unwrap();
        drop(v);
        let v = VaultSession::unlock_at(&path, "old master", FAST).unwrap();
        assert_eq!(v.data.entries.len(), 2);
        assert!(matches!(
            VaultSession::unlock_at(&path, "wrong", FAST),
            Err(VaultError::BadPassword)
        ));
    }

    #[test]
    fn weaker_v2_params_are_upgraded() {
        let tmp = TempVault::new("upgrade");
        let path = tmp.path();
        create_at(&path, "password1", FAST).unwrap();
        let stronger = KdfParams { m_cost: 128, ..FAST };
        VaultSession::unlock_at(&path, "password1", stronger).unwrap();
        let header = parse_header(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(header.kdf, stronger);
        VaultSession::unlock_at(&path, "password1", stronger).unwrap();
    }

    #[test]
    fn failed_migration_keeps_old_file_and_session() {
        let tmp = TempVault::new("readonly");
        let path = tmp.path();
        let data = VaultData {
            version: 1,
            entries: vec![test_entry("x", "https://old.test")],
            never_save: Vec::new(),
        };
        write_v1(&path, "old master", &data);
        let before = std::fs::read(&path).unwrap();
        let dir = path.parent().unwrap();
        // Read-only dir: the temp file can't be created, so the rename never happens.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = VaultSession::unlock_at(&path, "old master", FAST);
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let v = result.unwrap();
        assert_eq!(v.data.entries.len(), 1);
        if std::fs::read(&path).unwrap() != before {
            // Root ignores directory permissions, so the upgrade went through.
            return;
        }
        // Next save works with the old key and writes v2 with the v1 params.
        v.save().unwrap();
        let header = parse_header(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!((header.version, header.kdf), (VERSION_V2, KdfParams::V1));
    }

    #[test]
    fn vault_file_is_0600_and_no_temp_left() {
        let tmp = TempVault::new("mode");
        let path = tmp.path();
        let v = create_at(&path, "password1", FAST).unwrap();
        assert_eq!(mode(&path), 0o600);
        v.save().unwrap();
        assert_eq!(mode(&path), 0o600);
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["vault.enc"]);
    }

    #[test]
    fn json_buffer_is_exact_size() {
        let data = VaultData {
            version: 1,
            entries: vec![test_entry("a", "https://a.test"), test_entry("b", "https://b.test")],
            never_save: vec!["https://\"quoted\".test".into()],
        };
        let buf = to_json(&data).unwrap();
        assert_eq!(buf.len(), buf.capacity());
        assert_eq!(*buf, serde_json::to_vec(&data).unwrap());
    }

    #[test]
    fn current_params_are_valid() {
        assert_eq!(KdfParams::CURRENT.validate().unwrap(), KdfParams::CURRENT);
        assert_eq!(KdfParams::V1.validate().unwrap(), KdfParams::V1);
        assert!(KdfParams::V1.weaker_than(KdfParams::CURRENT));
    }

    /// `cargo test -p compositor --release kdf_timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn kdf_timing() {
        let start = Instant::now();
        derive_key("password", &[7; SALT_LEN], KdfParams::CURRENT).unwrap();
        eprintln!("Argon2id {:?}: {:?}", KdfParams::CURRENT, start.elapsed());
    }

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
        assert!(generate_password(64, false).bytes().all(|c| GEN_ALPHA.contains(&c)));
        let with_sym = GEN_ALPHA.iter().chain(GEN_SYM).copied().collect::<Vec<_>>();
        assert!(generate_password(128, true).bytes().all(|c| with_sym.contains(&c)));
    }

    #[test]
    fn uniform_index_rejects_biased_tail() {
        // n = 70: zone = 210, so 210..=255 must be redrawn.
        let mut bytes = [255u8, 210, 209].into_iter();
        assert_eq!(uniform_index(70, &mut || bytes.next().unwrap()), 209 % 70);
        // Every accepted byte maps to an index, each hit exactly zone/n times.
        let n = 70;
        let mut counts = vec![0u32; n];
        for b in 0..=255u8 {
            let mut once = Some(b);
            let mut next = || once.take().unwrap_or(0);
            if (b as usize) < 256 - 256 % n {
                counts[uniform_index(n, &mut next)] += 1;
            }
        }
        assert!(counts.iter().all(|&c| c == 3));
        assert_eq!(uniform_index(256, &mut || 255), 255);
        assert_eq!(uniform_index(1, &mut || 200), 0);
    }
}
