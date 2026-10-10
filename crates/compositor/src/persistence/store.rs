// Crash-safe JSON files under ~/.config/spatial-browser: written to a 0600
// temp file, fsynced, then renamed over the old one (whose last good copy is
// kept as `<name>.bak`). A file that fails to parse is moved aside, never
// overwritten, and the `.bak` is tried instead.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

fn sibling(path: &Path, prefix: &str, suffix: &str) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{prefix}{name}{suffix}"))
}

/// Create `dir` if needed and make it owner-only: it holds history,
/// sessions and the vault.
fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    if fs::metadata(dir)?.permissions().mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Replace `path` with `bytes` so a crash at any point leaves either the old
/// or the new contents, never a truncated file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    ensure_private_dir(dir)?;
    let tmp = sibling(path, ".", ".tmp");
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        // `mode` only applies on creation; a stale tmp may have other bits.
        file.set_permissions(Permissions::from_mode(0o600))?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if path.exists() {
        let bak = sibling(path, "", ".bak");
        let _ = fs::remove_file(&bak);
        // Hard link keeps `path` in place the whole time; best effort.
        if let Err(e) = fs::hard_link(path, &bak) {
            log::debug!("backup {bak:?}: {e}");
        }
    }
    fs::rename(&tmp, path)
}

/// Serialize and `write_atomic`, logging failures under `what`.
pub fn save_json<T: Serialize + ?Sized>(path: &Path, what: &str, value: &T) {
    match serde_json::to_vec_pretty(value) {
        Ok(bytes) => {
            if let Err(e) = write_atomic(path, &bytes) {
                log::warn!("{what} save: couldn't write {path:?}: {e}");
            }
        }
        Err(e) => log::warn!("{what} save: serialize failed: {e}"),
    }
}

/// Read `path` with `parse`. On a parse failure the file is moved to
/// `<name>.corrupt-<unix>` (so the next save can't destroy it) and the
/// `.bak` is tried. `None` when neither yields a value.
pub fn load_with<T>(path: &Path, what: &str, parse: impl Fn(&[u8]) -> Option<T>) -> Option<T> {
    match fs::read(path) {
        Ok(bytes) => {
            if let Some(value) = parse(&bytes) {
                return Some(value);
            }
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let aside = sibling(path, "", &format!(".corrupt-{stamp}"));
            match fs::rename(path, &aside) {
                Ok(()) => log::warn!("{what}: {path:?} unreadable, moved to {aside:?}; trying backup"),
                Err(e) => log::warn!("{what}: {path:?} unreadable and not moved aside: {e}"),
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            log::warn!("{what}: couldn't read {path:?}: {e}");
            return None;
        }
    }
    let bak = sibling(path, "", ".bak");
    let value = fs::read(&bak).ok().and_then(|bytes| parse(&bytes));
    if value.is_some() {
        log::warn!("{what}: restored from {bak:?}");
    }
    value
}

pub fn load_json<T: DeserializeOwned>(path: &Path, what: &str) -> Option<T> {
    load_with(path, what, |bytes| serde_json::from_slice(bytes).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "spatial-store-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip_with_private_modes_and_backup() {
        let dir = temp_dir("rt").join("cfg");
        let path = dir.join("a.json");
        save_json(&path, "t", &vec![1, 2]);
        save_json(&path, "t", &vec![3]);
        assert_eq!(load_json::<Vec<i32>>(&path, "t"), Some(vec![3]));
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        let bak: Vec<i32> = serde_json::from_slice(&fs::read(dir.join("a.json.bak")).unwrap()).unwrap();
        assert_eq!(bak, vec![1, 2]);
        assert!(!dir.join(".a.json.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_kept_aside_and_backup_used() {
        let dir = temp_dir("corrupt");
        let path = dir.join("b.json");
        save_json(&path, "t", &vec![7]);
        save_json(&path, "t", &vec![8]);
        fs::write(&path, b"{ truncated").unwrap();

        assert_eq!(load_json::<Vec<i32>>(&path, "t"), Some(vec![7]));
        assert!(!path.exists(), "corrupt file must not stay where a save would overwrite it");
        let aside: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("b.json.corrupt-"))
            .collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(fs::read(aside[0].path()).unwrap(), b"{ truncated");
    }

    #[test]
    fn missing_file_is_none() {
        let dir = temp_dir("missing");
        assert_eq!(load_json::<Vec<i32>>(&dir.join("none.json"), "t"), None);
    }

    #[test]
    fn stale_tmp_is_replaced() {
        let dir = temp_dir("tmp");
        let path = dir.join("c.json");
        fs::write(dir.join(".c.json.tmp"), b"garbage that is longer than the payload").unwrap();
        fs::set_permissions(dir.join(".c.json.tmp"), Permissions::from_mode(0o644)).unwrap();
        save_json(&path, "t", &1);
        assert_eq!(load_json::<i32>(&path, "t"), Some(1));
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
