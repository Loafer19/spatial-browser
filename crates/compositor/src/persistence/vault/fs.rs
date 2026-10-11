// Durable vault file writes and permission hygiene.

use super::*;

/// Replace `path` with `bytes` so a crash leaves either the old or the new
/// file: write a 0600 temp file, fsync, rename over, fsync the directory.
pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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

/// Vaults written before files were created 0600 inherited the umask
/// (usually 0644); tighten on read. Best effort — unlock still works.
pub(super) fn restrict_permissions(path: &Path) {
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
