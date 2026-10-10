// Crash-safe JSON files under ~/.config/spatial-browser: written to a 0600
// temp file, fsynced, then renamed over the old one. Settings-like files
// keep their last good copy as `<name>.bak`; history-like files don't, so
// clearing them really clears them. A file that fails to parse is moved
// aside, never overwritten, and the `.bak` is tried instead.
//
// Hot files (session, workspaces, history, ...) go through `save_json_bg`:
// a background writer, newest version per path wins, so fsync never
// stalls the frame loop. `flush` drains it (window close).

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

/// Whether a file keeps its previous version as `<name>.bak`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backup {
    Keep,
    /// Browsing traces (history, downloads, typed URLs): a removed entry
    /// must not live on in a backup.
    None,
}

/// Quarantined unreadable copies kept per file.
const KEEP_CORRUPT: usize = 3;

fn sibling(path: &Path, prefix: &str, suffix: &str) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{prefix}{name}{suffix}"))
}

/// Create `dir` if needed and make it owner-only: it holds history,
/// sessions and the vault. A chmod that fails (not our dir, read-only
/// mount) is logged, not fatal.
fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    if fs::metadata(dir)?.permissions().mode() & 0o777 != 0o700 {
        if let Err(e) = fs::set_permissions(dir, Permissions::from_mode(0o700)) {
            log::warn!("couldn't make {dir:?} private: {e}");
        }
    }
    Ok(())
}

/// Replace `path` with `bytes` so a crash at any point leaves either the old
/// or the new contents, never a truncated file. A symlinked file is written
/// through to its target (dotfile setups), not replaced by a regular file.
pub fn write_atomic(path: &Path, bytes: &[u8], backup: Backup) -> io::Result<()> {
    let resolved;
    let path = if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        resolved = fs::canonicalize(path)?;
        resolved.as_path()
    } else {
        path
    };
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
    let bak = sibling(path, "", ".bak");
    match backup {
        Backup::Keep if path.exists() => {
            let _ = fs::remove_file(&bak);
            // Hard link keeps `path` in place the whole time; best effort.
            if let Err(e) = fs::hard_link(path, &bak) {
                log::debug!("backup {bak:?}: {e}");
            }
        }
        Backup::Keep => {}
        Backup::None => {
            let _ = fs::remove_file(&bak);
        }
    }
    fs::rename(&tmp, path)
}

/// Serialize and `write_atomic` now, logging failures under `what`.
pub fn save_json<T: Serialize + ?Sized>(path: &Path, what: &str, value: &T, backup: Backup) {
    match serde_json::to_vec_pretty(value) {
        Ok(bytes) => {
            if let Err(e) = write_atomic(path, &bytes, backup) {
                log::warn!("{what} save: couldn't write {path:?}: {e}");
            }
        }
        Err(e) => log::warn!("{what} save: serialize failed: {e}"),
    }
}

struct Job {
    what: String,
    bytes: Vec<u8>,
    backup: Backup,
}

#[derive(Default)]
struct Queue {
    jobs: HashMap<PathBuf, Job>,
    writing: usize,
}

struct Writer {
    queue: Mutex<Queue>,
    changed: Condvar,
}

fn writer() -> &'static Writer {
    static WRITER: OnceLock<&'static Writer> = OnceLock::new();
    WRITER.get_or_init(|| {
        let writer: &'static Writer = Box::leak(Box::new(Writer {
            queue: Mutex::new(Queue::default()),
            changed: Condvar::new(),
        }));
        let spawned = std::thread::Builder::new()
            .name("config-writer".into())
            .spawn(move || loop {
                let (path, job) = {
                    let mut queue = writer.queue.lock().unwrap_or_else(|e| e.into_inner());
                    while queue.jobs.is_empty() {
                        queue = writer
                            .changed
                            .wait(queue)
                            .unwrap_or_else(|e| e.into_inner());
                    }
                    let path = queue.jobs.keys().next().cloned().expect("non-empty");
                    let job = queue.jobs.remove(&path).expect("present");
                    queue.writing += 1;
                    (path, job)
                };
                if let Err(e) = write_atomic(&path, &job.bytes, job.backup) {
                    log::warn!("{} save: couldn't write {path:?}: {e}", job.what);
                }
                let mut queue = writer.queue.lock().unwrap_or_else(|e| e.into_inner());
                queue.writing -= 1;
                writer.changed.notify_all();
            });
        if let Err(e) = spawned {
            log::warn!("config writer thread: {e}; saves stay on the UI thread");
        }
        writer
    })
}

/// Serialize now (cheap) and leave the fsynced write to the background
/// writer. A newer save of the same path replaces a queued older one.
pub fn save_json_bg<T: Serialize + ?Sized>(path: &Path, what: &str, value: &T, backup: Backup) {
    let bytes = match serde_json::to_vec_pretty(value) {
        Ok(bytes) => bytes,
        Err(e) => return log::warn!("{what} save: serialize failed: {e}"),
    };
    let w = writer();
    let mut queue = w.queue.lock().unwrap_or_else(|e| e.into_inner());
    queue.jobs.insert(
        path.to_path_buf(),
        Job {
            what: what.to_string(),
            bytes,
            backup,
        },
    );
    w.changed.notify_all();
}

/// Block until every queued background save is on disk.
pub fn flush() {
    let w = writer();
    let mut queue = w.queue.lock().unwrap_or_else(|e| e.into_inner());
    while !queue.jobs.is_empty() || queue.writing > 0 {
        let (next, timeout) = w
            .changed
            .wait_timeout(queue, std::time::Duration::from_secs(5))
            .unwrap_or_else(|e| e.into_inner());
        queue = next;
        if timeout.timed_out() {
            log::warn!(
                "config writer: flush timed out with {} pending",
                queue.jobs.len()
            );
            return;
        }
    }
}

/// Drop all but the newest `KEEP_CORRUPT` quarantined copies of `path`.
fn prune_corrupt(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let prefix = format!("{}.corrupt-", name.to_string_lossy());
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut aside: Vec<(u64, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let stamp = file.strip_prefix(&prefix)?.parse().ok()?;
            Some((stamp, e.path()))
        })
        .collect();
    aside.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, old) in aside.into_iter().skip(KEEP_CORRUPT) {
        let _ = fs::remove_file(old);
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
                Ok(()) => {
                    log::warn!("{what}: {path:?} unreadable, moved to {aside:?}; trying backup")
                }
                Err(e) => log::warn!("{what}: {path:?} unreadable and not moved aside: {e}"),
            }
            prune_corrupt(path);
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
        save_json(&path, "t", &vec![1, 2], Backup::Keep);
        save_json(&path, "t", &vec![3], Backup::Keep);
        assert_eq!(load_json::<Vec<i32>>(&path, "t"), Some(vec![3]));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let bak: Vec<i32> =
            serde_json::from_slice(&fs::read(dir.join("a.json.bak")).unwrap()).unwrap();
        assert_eq!(bak, vec![1, 2]);
        assert!(!dir.join(".a.json.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_kept_aside_and_backup_used() {
        let dir = temp_dir("corrupt");
        let path = dir.join("b.json");
        save_json(&path, "t", &vec![7], Backup::Keep);
        save_json(&path, "t", &vec![8], Backup::Keep);
        fs::write(&path, b"{ truncated").unwrap();

        assert_eq!(load_json::<Vec<i32>>(&path, "t"), Some(vec![7]));
        assert!(
            !path.exists(),
            "corrupt file must not stay where a save would overwrite it"
        );
        let aside: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("b.json.corrupt-")
            })
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
        fs::write(
            dir.join(".c.json.tmp"),
            b"garbage that is longer than the payload",
        )
        .unwrap();
        fs::set_permissions(dir.join(".c.json.tmp"), Permissions::from_mode(0o644)).unwrap();
        save_json(&path, "t", &1, Backup::Keep);
        assert_eq!(load_json::<i32>(&path, "t"), Some(1));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn no_backup_files_drop_their_bak() {
        let dir = temp_dir("nobak");
        let path = dir.join("h.json");
        save_json(&path, "t", &vec![1], Backup::Keep);
        save_json(&path, "t", &vec![2], Backup::Keep);
        assert!(dir.join("h.json.bak").exists());
        save_json(&path, "t", &Vec::<i32>::new(), Backup::None);
        assert!(
            !dir.join("h.json.bak").exists(),
            "cleared history must not survive in .bak"
        );
    }

    #[test]
    fn background_saves_coalesce_and_flush() {
        let dir = temp_dir("bg");
        let path = dir.join("s.json");
        for i in 0..50 {
            save_json_bg(&path, "t", &i, Backup::Keep);
        }
        flush();
        assert_eq!(load_json::<i32>(&path, "t"), Some(49));
    }

    #[test]
    fn symlinked_file_is_written_through() {
        let dir = temp_dir("link");
        let real = dir.join("real.json");
        let link = dir.join("link.json");
        fs::write(&real, b"1").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        save_json(&link, "t", &2, Backup::Keep);
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(load_json::<i32>(&real, "t"), Some(2));
    }

    #[test]
    fn only_newest_corrupt_copies_are_kept() {
        let dir = temp_dir("prune");
        let path = dir.join("p.json");
        for stamp in [1u64, 2, 3, 4, 5] {
            fs::write(dir.join(format!("p.json.corrupt-{stamp}")), b"x").unwrap();
        }
        prune_corrupt(&path);
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            ["p.json.corrupt-3", "p.json.corrupt-4", "p.json.corrupt-5"]
        );
    }
}
