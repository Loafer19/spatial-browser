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
        .encrypt(
            Nonce::from_slice(&nonce),
            serde_json::to_vec(data).unwrap().as_ref(),
        )
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
fn header_is_bound_as_aad() {
    // Decrypting with the right key but without the header as AAD must
    // fail — otherwise header fields could be swapped freely.
    let tmp = TempVault::new("aad");
    let path = tmp.path();
    create_at(&path, "password1", FAST).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let header = parse_header(&bytes).unwrap();
    let key = derive_key("password1", &header.salt, header.kdf).unwrap();
    let cipher = Aes256Gcm::new_from_slice(&key.0).unwrap();
    let open = |aad: &[u8]| {
        cipher.decrypt(
            Nonce::from_slice(&header.nonce),
            Payload {
                msg: &bytes[header.len..],
                aad,
            },
        )
    };
    assert!(open(&bytes[..header.len]).is_ok());
    assert!(open(&[]).is_err());
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
    let stronger = KdfParams {
        m_cost: 128,
        ..FAST
    };
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
        entries: vec![
            test_entry("a", "https://a.test"),
            test_entry("b", "https://b.test"),
        ],
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
    assert_eq!(
        normalize_origin("https://Example.COM/a?b#c"),
        "https://example.com"
    );
    assert_eq!(
        normalize_origin("http://example.com:8080/x"),
        "http://example.com:8080"
    );
    assert_eq!(normalize_origin("example.com/login"), "https://example.com");
    assert_eq!(normalize_origin("  https://a.test  "), "https://a.test");
}

#[test]
fn host_key_ignores_scheme_and_www_only() {
    assert_eq!(
        host_key("https://www.example.com"),
        host_key("http://example.com")
    );
    assert_ne!(
        host_key("https://example.com:8443"),
        host_key("https://example.com")
    );
    assert_ne!(
        host_key("https://login.example.com"),
        host_key("https://example.com")
    );
}

#[test]
fn normalize_origin_rejects_non_web_urls() {
    for bad in [
        "data:text/html,https://bank.test",
        "data:text/html;base64,PGgxPg==",
        "javascript:alert(1)",
        "about:blank",
        "mailto:a@b.test",
        "1http://x.test",
        "https://",
        "",
    ] {
        assert_eq!(normalize_origin(bad), "", "{bad}");
    }
    assert_eq!(
        normalize_origin("localhost:3000/x"),
        "https://localhost:3000"
    );
    assert_eq!(
        normalize_origin("HTTPS://Bank.Test/login"),
        "https://bank.test"
    );
    assert_eq!(
        normalize_origin("chrome-extension://abc/x"),
        "chrome-extension://abc"
    );
    assert_eq!(host_key("data:text/html,https://bank.test"), "");
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
    assert_ne!(
        host_key("https://bank.test@evil.test/"),
        host_key("https://bank.test")
    );
}

#[test]
fn entries_for_origin_matches_host_key() {
    let v = VaultSession::for_test(vec![
        test_entry("1", "https://www.a.test"),
        test_entry("2", "https://b.test"),
    ]);
    let ids: Vec<_> = v
        .entries_for_origin("http://a.test/login")
        .iter()
        .map(|e| e.id.as_str())
        .collect();
    assert_eq!(ids, ["1"]);
}

#[test]
fn generate_password_clamps_length_and_respects_symbols() {
    assert_eq!(generate_password(1, true).len(), 8);
    assert_eq!(generate_password(500, true).len(), 128);
    assert!(generate_password(64, false)
        .bytes()
        .all(|c| GEN_ALPHA.contains(&c)));
    let with_sym = GEN_ALPHA.iter().chain(GEN_SYM).copied().collect::<Vec<_>>();
    assert!(generate_password(128, true)
        .bytes()
        .all(|c| with_sym.contains(&c)));
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
