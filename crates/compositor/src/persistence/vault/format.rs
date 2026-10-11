// Vault file format and crypto: KDF params, header parsing, sealing.

use super::*;

pub(super) const MAGIC: &[u8; 4] = b"SBVT";
pub(super) const VERSION_V1: u8 = 1;
pub(super) const VERSION_V2: u8 = 2;
pub(super) const SALT_LEN: usize = 16;
pub(super) const NONCE_LEN: usize = 12;
pub(super) const KEY_LEN: usize = 32;
pub(super) const TAG_LEN: usize = 16;
pub(super) const V1_HEADER_LEN: usize = 4 + 1 + SALT_LEN + NONCE_LEN;
pub(super) const V2_HEADER_LEN: usize = 4 + 1 + 3 * 4 + SALT_LEN + NONCE_LEN;

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
    pub(super) const V1: Self = Self {
        m_cost: Params::DEFAULT_M_COST,
        t_cost: Params::DEFAULT_T_COST,
        p_cost: Params::DEFAULT_P_COST,
    };

    /// Bounds for header values: a corrupt or tampered file must not make
    /// unlock allocate gigabytes or spin for minutes.
    pub(super) fn validate(self) -> Result<Self, VaultError> {
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

    pub(super) fn weaker_than(self, other: Self) -> bool {
        self.m_cost < other.m_cost || self.t_cost < other.t_cost
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub(super) struct KeyMaterial(pub(super) [u8; KEY_LEN]);

pub(super) fn derive_key(
    password: &str,
    salt: &[u8; SALT_LEN],
    kdf: KdfParams,
) -> Result<KeyMaterial, VaultError> {
    let params = Params::new(kdf.m_cost, kdf.t_cost, kdf.p_cost, Some(KEY_LEN))
        .map_err(|_| VaultError::Corrupt("bad KDF parameters"))?;
    // Hash straight into the zeroize-on-drop buffer; no stray stack copy.
    let mut key = KeyMaterial([0u8; KEY_LEN]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut key.0)
        .map_err(|_| VaultError::Corrupt("argon2 failed"))?;
    Ok(key)
}

pub(super) fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    rand::rng().fill_bytes(&mut salt);
    salt
}

pub(super) struct Header {
    pub(super) version: u8,
    pub(super) kdf: KdfParams,
    pub(super) salt: [u8; SALT_LEN],
    pub(super) nonce: [u8; NONCE_LEN],
    /// Bytes before the ciphertext (the AAD for v2).
    pub(super) len: usize,
}

pub(super) fn read_u32(bytes: &[u8], at: usize) -> u32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&bytes[at..at + 4]);
    u32::from_le_bytes(b)
}

pub(super) fn parse_header(bytes: &[u8]) -> Result<Header, VaultError> {
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
pub(super) fn to_json(data: &VaultData) -> Result<Zeroizing<Vec<u8>>, VaultError> {
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
pub(super) fn seal(
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
