// Accept downloads (CEF default cancels), save under ~/Downloads with unique
// names, track progress for the live downloads page, queue completions for
// the compositor.
//
// Downloads are never left executable, and executables/scripts/unknown types
// are never handed to xdg-open: the downloads page "Open" reveals their folder
// instead (see `download_open_target`).

use cef::{self, rc::Rc, *};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Completed download for downloads.json + desktop notification.
pub struct CompletedDownload {
    pub url: String,
    pub path: String,
    /// Server MIME type (empty if CEF didn't get one).
    pub mime: String,
}

/// Where a download is in its life, as the downloads page shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadState {
    InProgress,
    Paused,
    Complete,
    Canceled,
    /// Network or disk error. Stays on the page (with its callback) until it
    /// is resumed or dismissed.
    Interrupted,
}

impl DownloadState {
    /// Stable name for the page script (`data-state`).
    pub fn as_str(self) -> &'static str {
        match self {
            DownloadState::InProgress => "in-progress",
            DownloadState::Paused => "paused",
            DownloadState::Complete => "complete",
            DownloadState::Canceled => "canceled",
            DownloadState::Interrupted => "interrupted",
        }
    }

    /// CEF is done with it: no more updates, callback dropped.
    pub fn is_final(self) -> bool {
        matches!(self, DownloadState::Complete | DownloadState::Canceled)
    }

    fn of(complete: bool, canceled: bool, interrupted: bool, paused: bool) -> Self {
        if complete {
            DownloadState::Complete
        } else if canceled {
            DownloadState::Canceled
        } else if interrupted {
            DownloadState::Interrupted
        } else if paused {
            DownloadState::Paused
        } else {
            DownloadState::InProgress
        }
    }
}

/// One download as of its latest `on_download_updated`.
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadProgress {
    /// CEF download id, unique for this run.
    pub id: u32,
    pub url: String,
    pub suggested_name: String,
    /// Empty until `on_before_download` has picked the path.
    pub full_path: String,
    pub received_bytes: u64,
    /// `None` when the server sent no length.
    pub total_bytes: Option<u64>,
    /// Bytes per second (CEF's running estimate).
    pub speed: u64,
    /// 0–100; `None` when the total is unknown.
    pub percent: Option<u8>,
    pub state: DownloadState,
    pub mime: String,
    /// Unix seconds.
    pub started_at: i64,
}

impl DownloadProgress {
    /// The saved file's name, else CEF's suggestion, else "download".
    pub fn file_name(&self) -> &str {
        [&self.full_path, &self.suggested_name]
            .into_iter()
            .find_map(|p| {
                Path::new(p)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .filter(|f| !f.is_empty())
            })
            .unwrap_or("download")
    }
}

/// What moved since the compositor last took `PENDING_DOWNLOAD_CHANGES`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DownloadChanges {
    /// Bytes, speed or path of some download.
    pub progress: bool,
    /// A download started, paused, resumed, finished, failed or was dismissed.
    pub state: bool,
}

impl DownloadChanges {
    fn merge(&mut self, other: DownloadChanges) {
        self.progress |= other.progress;
        self.state |= other.state;
    }
}

/// Live control from the downloads page (`download://cancel|pause|resume/<id>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadControl {
    Cancel,
    Pause,
    Resume,
}

thread_local! {
    pub static PENDING_DOWNLOADS: RefCell<Vec<CompletedDownload>> = const { RefCell::new(Vec::new()) };
    /// Set by every download update; the compositor takes it each tick and
    /// pushes `active_downloads()` to open downloads tiles.
    pub static PENDING_DOWNLOAD_CHANGES: RefCell<DownloadChanges> =
        const { RefCell::new(DownloadChanges { progress: false, state: false }) };
    // Not yet complete or canceled, by id (= start order). Interrupted ones
    // stay until resumed or dismissed.
    static ACTIVE_DOWNLOADS: RefCell<BTreeMap<u32, DownloadProgress>> =
        const { RefCell::new(BTreeMap::new()) };
    // Callback (cancel/pause/resume) of every download in ACTIVE_DOWNLOADS.
    static DOWNLOAD_CALLBACKS: RefCell<HashMap<u32, DownloadItemCallback>> =
        RefCell::new(HashMap::new());
    // on_download_updated repeats after complete/cancel — dedupe by download id.
    static ENDED_DOWNLOADS: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
    // Completed this run with a risky server MIME type. Not persisted; after a
    // restart `is_risky_file` still catches them by extension or content.
    static RISKY_BY_MIME: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

/// Downloads still running, paused or interrupted, oldest first.
pub fn active_downloads() -> Vec<DownloadProgress> {
    ACTIVE_DOWNLOADS.with_borrow(|active| active.values().cloned().collect())
}

/// Cancel, pause or resume download `id`. False if it is unknown or over.
/// Cancel also dismisses an interrupted download from the page.
pub fn control_download(id: u32, control: DownloadControl) -> bool {
    let Some(callback) = DOWNLOAD_CALLBACKS.with_borrow(|callbacks| callbacks.get(&id).cloned())
    else {
        return false;
    };
    // No borrow held here: CEF may run on_download_updated re-entrantly.
    match control {
        DownloadControl::Cancel => callback.cancel(),
        DownloadControl::Pause => callback.pause(),
        DownloadControl::Resume => callback.resume(),
    }
    if control == DownloadControl::Cancel {
        // An interrupted download may never report the cancel.
        end_download(id);
        let removed = ACTIVE_DOWNLOADS.with_borrow_mut(|active| active.remove(&id).is_some());
        if removed {
            note_download_changes(DownloadChanges {
                progress: false,
                state: true,
            });
        }
    }
    true
}

fn end_download(id: u32) {
    ENDED_DOWNLOADS.with_borrow_mut(|ended| ended.insert(id));
    DOWNLOAD_CALLBACKS.with_borrow_mut(|callbacks| callbacks.remove(&id));
}

fn note_download_changes(changes: DownloadChanges) {
    PENDING_DOWNLOAD_CHANGES.with_borrow_mut(|pending| pending.merge(changes));
}

/// Fold `progress` into the active map; says what the page must redraw.
fn track_update(
    active: &mut BTreeMap<u32, DownloadProgress>,
    progress: DownloadProgress,
) -> DownloadChanges {
    let previous = active.get(&progress.id);
    let changes = DownloadChanges {
        state: previous.is_none_or(|p| p.state != progress.state),
        progress: previous.is_none_or(|p| {
            p.received_bytes != progress.received_bytes
                || p.total_bytes != progress.total_bytes
                || p.speed != progress.speed
                || p.full_path != progress.full_path
        }),
    };
    if progress.state.is_final() {
        active.remove(&progress.id);
    } else {
        active.insert(progress.id, progress);
    }
    changes
}

/// CEF base::Time (microseconds since 1601-01-01 UTC) → Unix seconds.
fn basetime_to_unix(micros: i64) -> Option<i64> {
    const WINDOWS_TO_UNIX_SECS: i64 = 11_644_473_600;
    (micros > 0).then(|| micros / 1_000_000 - WINDOWS_TO_UNIX_SECS)
}

/// (received, total, percent). CEF reports an unknown size as 0 or -1 and
/// an unknown percent as -1.
fn sizes(received: i64, total: i64, percent: i32) -> (u64, Option<u64>, Option<u8>) {
    let received = received.max(0) as u64;
    let total = (total > 0).then_some(total as u64);
    let percent = total.map(|total| match u8::try_from(percent) {
        Ok(p) if p <= 100 => p,
        _ => (received.min(total) * 100 / total) as u8,
    });
    (received, total, percent)
}

fn progress_of(item: &DownloadItem) -> DownloadProgress {
    let (received_bytes, total_bytes, percent) = sizes(
        item.received_bytes(),
        item.total_bytes(),
        item.percent_complete(),
    );
    let started_at = basetime_to_unix(item.start_time().val).unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    DownloadProgress {
        id: item.id(),
        url: CefString::from(&item.url()).to_string(),
        suggested_name: CefString::from(&item.suggested_file_name()).to_string(),
        full_path: CefString::from(&item.full_path()).to_string(),
        received_bytes,
        total_bytes,
        speed: item.current_speed().max(0) as u64,
        percent,
        state: DownloadState::of(
            item.is_complete() != 0,
            item.is_canceled() != 0,
            item.is_interrupted() != 0,
            item.is_paused() != 0,
        ),
        mime: CefString::from(&item.mime_type()).to_string(),
        started_at,
    }
}

/// Extensions that run code when opened (or install something) on Linux, plus
/// Windows executables that Wine would happily launch via xdg-open.
const RISKY_EXTENSIONS: &[&str] = &[
    // Linux executables, installers, launchers
    "appimage",
    "run",
    "bin",
    "elf",
    "out",
    "so",
    "ko",
    "desktop",
    "deb",
    "rpm",
    "snap",
    "flatpak",
    "flatpakref",
    "flatpakrepo",
    "pkg",
    "x86_64",
    "aarch64",
    // Scripts
    "sh",
    "bash",
    "zsh",
    "csh",
    "ksh",
    "fish",
    "command",
    "py",
    "pyw",
    "pyc",
    "pl",
    "rb",
    "php",
    "lua",
    "tcl",
    "jar",
    "jnlp",
    // Windows (Wine)
    "exe",
    "msi",
    "msp",
    "com",
    "bat",
    "cmd",
    "ps1",
    "vbs",
    "vbe",
    "wsf",
    "scr",
    "pif",
    "lnk",
    "reg",
    "cpl",
    "hta",
];

/// MIME types (parameters stripped, lowercase) that mean "this is a program".
/// `application/octet-stream` is here because servers use it for exactly the
/// binaries we care about; the name/content checks decide the rest.
const RISKY_MIME_TYPES: &[&str] = &[
    "application/octet-stream",
    "application/x-executable",
    "application/x-elf",
    "application/x-sharedlib",
    "application/x-pie-executable",
    "application/x-mach-binary",
    "application/x-msdownload",
    "application/x-msdos-program",
    "application/x-ms-installer",
    "application/x-msi",
    "application/x-dosexec",
    "application/vnd.microsoft.portable-executable",
    "application/x-sh",
    "application/x-shellscript",
    "text/x-shellscript",
    "text/x-sh",
    "application/x-csh",
    "application/x-python",
    "application/x-python-code",
    "text/x-python",
    "text/x-script.python",
    "application/x-perl",
    "text/x-perl",
    "application/x-ruby",
    "text/x-ruby",
    "application/java-archive",
    "application/x-java-archive",
    "application/x-java-jnlp-file",
    "application/x-deb",
    "application/vnd.debian.binary-package",
    "application/x-rpm",
    "application/x-redhat-package-manager",
    "application/x-desktop",
    "application/x-appimage",
    "application/vnd.appimage",
    "application/vnd.flatpak",
    "application/vnd.flatpak.ref",
    "application/vnd.snap",
];

/// True for executables, scripts, installers and files with no extension, by
/// file name and (when known) server MIME type. Such downloads are kept, but
/// "Open" reveals their folder instead of running them through xdg-open.
pub fn is_risky_download(name: &str, mime: Option<&str>) -> bool {
    // "evil.sh." / "evil.sh " → judge by what a file manager would see.
    let trimmed = name.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    let file_name = Path::new(trimmed)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("");
    let risky_name = match Path::new(file_name).extension().and_then(|e| e.to_str()) {
        None => true,
        Some(ext) => {
            let ext = ext.to_ascii_lowercase();
            RISKY_EXTENSIONS.contains(&ext.as_str())
        }
    };
    let risky_mime = mime.is_some_and(|mime| {
        let essence = mime
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        RISKY_MIME_TYPES.contains(&essence.as_str())
    });
    risky_name || risky_mime
}

/// `is_risky_download` by name, plus content sniffing (ELF, PE, shebang) and
/// the exec bit, for a file already on disk whose MIME type we no longer have.
pub fn is_risky_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
    if is_risky_download(name, None) {
        return true;
    }
    if RISKY_BY_MIME.with_borrow(|risky| risky.contains(path)) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(meta) if meta.permissions().mode() & 0o111 != 0 => return true,
            Ok(_) => {}
            // Can't inspect it → don't hand it to xdg-open.
            Err(_) => return true,
        }
    }
    let mut magic = [0u8; 4];
    let read = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut magic))
        .unwrap_or(0);
    let magic = &magic[..read];
    magic.starts_with(b"\x7fELF") || magic.starts_with(b"#!") || magic.starts_with(b"MZ")
}

/// Canonical `path` if it resolves (symlinks included) to a file inside `dir`.
pub fn contained_download_path(dir: &Path, path: &Path) -> Option<PathBuf> {
    let dir = dir.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    (path != dir && path.starts_with(&dir) && path.is_file()).then_some(path)
}

/// What the downloads page "Open" should pass to xdg-open for a recorded
/// download: the file itself for ordinary types, its folder for risky ones,
/// or `None` if the record points outside ~/Downloads or the file is gone.
pub fn download_open_target(recorded_path: &str) -> Option<PathBuf> {
    let path = contained_download_path(&downloads_dir(), Path::new(recorded_path))?;
    if is_risky_file(&path) {
        path.parent().map(Path::to_path_buf)
    } else {
        Some(path)
    }
}

/// What "Show in folder" passes to xdg-open: the folder holding the
/// recorded file if it is still inside ~/Downloads, else ~/Downloads itself.
/// Always a directory, so xdg-open never runs anything.
pub fn download_folder_target(recorded_path: &str) -> PathBuf {
    let dir = downloads_dir();
    contained_download_path(&dir, Path::new(recorded_path))
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or(dir)
}

/// Clear every exec bit: nothing downloaded should be runnable by default.
fn strip_exec_bits(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        let mode = meta.permissions().mode();
        if mode & 0o111 != 0 {
            let perms = std::fs::Permissions::from_mode(mode & !0o111);
            if let Err(e) = std::fs::set_permissions(path, perms) {
                log::warn!("couldn't clear exec bit on download {path:?}: {e}");
            }
        }
    }
}

pub fn downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    let dir = PathBuf::from(home).join("Downloads");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// `name.pdf`, then `name (1).pdf`, … — never overwrite an existing file.
fn unique_download_path(dir: &Path, suggested_name: &str) -> PathBuf {
    // Only ever the last component: a name with '/' or '..' stays in `dir`.
    let suggested_name = Path::new(suggested_name)
        .file_name()
        .and_then(|f| f.to_str())
        .filter(|f| !f.is_empty())
        .unwrap_or("download");
    let suggested = Path::new(suggested_name);
    let stem = suggested
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("download");
    let ext = suggested.extension().and_then(|s| s.to_str());
    let mut candidate = dir.join(suggested_name);
    let mut n = 1;
    while candidate.exists() {
        candidate = dir.join(match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        });
        n += 1;
    }
    candidate
}

#[derive(Clone)]
pub struct OsrDownloadHandler {}

wrap_download_handler! {
    pub struct DownloadHandlerBuilder {
        handler: OsrDownloadHandler,
    }

    impl DownloadHandler {
        // Unimplemented default cancels every download.
        fn can_download(
            &self,
            _browser: Option<&mut Browser>,
            _url: Option<&CefString>,
            _request_method: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            true as _
        }

        // Same story: returning false here falls back to "default
        // handling", which in a windowless/OSR embedding has no download
        // shelf or save dialog to fall back to, so it just cancels. We
        // take it over ourselves: pick a path under ~/Downloads and
        // continue with no native save-as dialog.
        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            _download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(callback) = callback else {
                return false as _;
            };
            let name = suggested_name.map(|s| s.to_string()).unwrap_or_default();
            let path = unique_download_path(&downloads_dir(), &name);
            let path_string = path.to_string_lossy().into_owned();
            let cef_path: CefString = path_string.as_str().into();
            callback.cont(Some(&cef_path), false as _);
            true as _
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(item) = download_item else {
                return;
            };
            let id = item.id();
            if ENDED_DOWNLOADS.with_borrow(|ended| ended.contains(&id)) {
                return;
            }
            let progress = progress_of(item);
            let state = progress.state;
            if state.is_final() {
                end_download(id);
            } else if let Some(callback) = callback {
                let callback = callback.clone();
                DOWNLOAD_CALLBACKS.with_borrow_mut(|callbacks| callbacks.insert(id, callback));
            }
            let completed = (state == DownloadState::Complete).then(|| {
                (
                    progress.url.clone(),
                    progress.full_path.clone(),
                    progress.mime.clone(),
                )
            });
            let changes = ACTIVE_DOWNLOADS.with_borrow_mut(|active| track_update(active, progress));
            note_download_changes(changes);
            let Some((url, path, mime)) = completed else {
                return;
            };
            let path_buf = PathBuf::from(&path);
            strip_exec_bits(&path_buf);
            let name = path_buf.file_name().and_then(|f| f.to_str()).unwrap_or("");
            if is_risky_download(name, Some(&mime)) {
                log::info!("download {path:?} ({mime}) is executable/unknown; Open will reveal its folder");
                let canonical = path_buf.canonicalize().unwrap_or(path_buf);
                RISKY_BY_MIME.with_borrow_mut(|risky| risky.insert(canonical));
            }
            PENDING_DOWNLOADS.with_borrow_mut(|pending| {
                pending.push(CompletedDownload { url, path, mime });
            });
        }
    }
}

impl DownloadHandlerBuilder {
    pub fn build(handler: OsrDownloadHandler) -> cef::DownloadHandler {
        Self::new(handler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fresh empty dir under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "spatial-dl-test-{tag}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn risky_by_extension() {
        for name in [
            "install.sh",
            "Tool.AppImage",
            "setup.run",
            "fw.bin",
            "evil.desktop",
            "setup.exe",
            "pkg.msi",
            "app.deb",
            "app.rpm",
            "script.py",
            "app.jar",
            "invoice.pdf.exe",
            "trailing.sh.",
            "trailing.sh ",
            "UPPER.SH",
            "dir/inner.sh",
        ] {
            assert!(is_risky_download(name, None), "{name} should be risky");
        }
    }

    #[test]
    fn risky_without_extension() {
        assert!(is_risky_download("payload", None));
        assert!(is_risky_download(".bashrc", None));
        assert!(is_risky_download("", None));
    }

    #[test]
    fn safe_common_types() {
        for name in [
            "report.pdf",
            "photo.JPG",
            "data.csv",
            "archive.zip",
            "src.tar.gz",
            "notes.txt",
            "slides.pptx",
            "song.mp3",
            "movie.mkv",
            "page.html",
        ] {
            assert!(!is_risky_download(name, None), "{name} should be safe");
            assert!(!is_risky_download(name, Some("")), "{name} with empty mime");
        }
        assert!(!is_risky_download("report.pdf", Some("application/pdf")));
        assert!(!is_risky_download(
            "data.csv",
            Some("text/csv; charset=utf-8")
        ));
    }

    #[test]
    fn risky_by_mime() {
        assert!(is_risky_download(
            "report.pdf",
            Some("application/octet-stream")
        ));
        assert!(is_risky_download("x.txt", Some("application/x-executable")));
        assert!(is_risky_download(
            "x.txt",
            Some("Application/X-Sh; charset=binary")
        ));
        assert!(is_risky_download(
            "x.zip",
            Some("application/vnd.debian.binary-package")
        ));
    }

    #[test]
    fn unique_path_stays_in_dir() {
        let tmp = TempDir::new("unique");
        assert_eq!(
            unique_download_path(&tmp.0, "../../etc/passwd"),
            tmp.0.join("passwd")
        );
        assert_eq!(unique_download_path(&tmp.0, ""), tmp.0.join("download"));
        std::fs::write(tmp.0.join("a.pdf"), b"x").unwrap();
        assert_eq!(
            unique_download_path(&tmp.0, "a.pdf"),
            tmp.0.join("a (1).pdf")
        );
    }

    #[test]
    fn containment() {
        let tmp = TempDir::new("contain");
        let downloads = tmp.0.join("Downloads");
        let outside = tmp.0.join("outside");
        std::fs::create_dir_all(downloads.join("sub")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let inside_file = downloads.join("sub/ok.pdf");
        let outside_file = outside.join("secret.txt");
        std::fs::write(&inside_file, b"%PDF").unwrap();
        std::fs::write(&outside_file, b"secret").unwrap();

        assert_eq!(
            contained_download_path(&downloads, &inside_file),
            Some(inside_file.canonicalize().unwrap())
        );
        // Traversal out of the dir.
        assert_eq!(
            contained_download_path(&downloads, &downloads.join("../outside/secret.txt")),
            None
        );
        assert_eq!(contained_download_path(&downloads, &outside_file), None);
        // The dir itself, and missing files.
        assert_eq!(contained_download_path(&downloads, &downloads), None);
        assert_eq!(
            contained_download_path(&downloads, &downloads.join("gone.pdf")),
            None
        );
        // Sibling with a shared prefix ("Downloads-evil") is not inside.
        let sibling = tmp.0.join("Downloads-evil");
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sibling.join("x.pdf"), b"x").unwrap();
        assert_eq!(
            contained_download_path(&downloads, &sibling.join("x.pdf")),
            None
        );
        // Symlink inside pointing outside.
        #[cfg(unix)]
        {
            let link = downloads.join("link.txt");
            std::os::unix::fs::symlink(&outside_file, &link).unwrap();
            assert_eq!(contained_download_path(&downloads, &link), None);
        }
    }

    #[test]
    fn risky_file_sniffs_content_and_exec_bit() {
        let tmp = TempDir::new("sniff");
        let pdf = tmp.0.join("ok.pdf");
        std::fs::write(&pdf, b"%PDF-1.7").unwrap();
        assert!(!is_risky_file(&pdf));

        let elf = tmp.0.join("fake.pdf");
        std::fs::write(&elf, b"\x7fELF\x02\x01").unwrap();
        assert!(is_risky_file(&elf));

        let script = tmp.0.join("notes.txt");
        std::fs::write(&script, b"#!/bin/sh\nrm -rf ~\n").unwrap();
        assert!(is_risky_file(&script));

        assert!(is_risky_file(&tmp.0.join("tool.AppImage")));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exec = tmp.0.join("doc.txt");
            std::fs::write(&exec, b"hello").unwrap();
            std::fs::set_permissions(&exec, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(is_risky_file(&exec));
            strip_exec_bits(&exec);
            let mode = std::fs::metadata(&exec).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o644);
            assert!(!is_risky_file(&exec));
        }
    }

    fn progress(id: u32, received: u64, state: DownloadState) -> DownloadProgress {
        DownloadProgress {
            id,
            url: "https://a.test/f.zip".into(),
            suggested_name: "f.zip".into(),
            full_path: String::new(),
            received_bytes: received,
            total_bytes: Some(100),
            speed: 10,
            percent: Some(received as u8),
            state,
            mime: "application/zip".into(),
            started_at: 0,
        }
    }

    #[test]
    fn state_precedence() {
        use DownloadState::*;
        assert_eq!(DownloadState::of(true, true, true, true), Complete);
        assert_eq!(DownloadState::of(false, true, true, true), Canceled);
        assert_eq!(DownloadState::of(false, false, true, true), Interrupted);
        assert_eq!(DownloadState::of(false, false, false, true), Paused);
        assert_eq!(DownloadState::of(false, false, false, false), InProgress);
        assert!(Complete.is_final() && Canceled.is_final());
        assert!(!InProgress.is_final() && !Paused.is_final() && !Interrupted.is_final());
    }

    #[test]
    fn track_update_reports_state_and_progress_and_drops_finished() {
        use DownloadState::*;
        let mut active = BTreeMap::new();
        let first = track_update(&mut active, progress(1, 0, InProgress));
        assert_eq!(
            first,
            DownloadChanges {
                progress: true,
                state: true
            }
        );
        // Same numbers again: nothing to redraw.
        assert_eq!(
            track_update(&mut active, progress(1, 0, InProgress)),
            DownloadChanges::default()
        );
        // Bytes moved.
        assert_eq!(
            track_update(&mut active, progress(1, 40, InProgress)),
            DownloadChanges {
                progress: true,
                state: false
            }
        );
        // Paused at the same byte count: a state change only.
        assert_eq!(
            track_update(&mut active, progress(1, 40, Paused)),
            DownloadChanges {
                progress: false,
                state: true
            }
        );
        assert_eq!(active[&1].state, Paused);
        // Interrupted stays listed; complete and canceled leave.
        track_update(&mut active, progress(2, 5, Interrupted));
        assert_eq!(active.len(), 2);
        assert!(track_update(&mut active, progress(1, 100, Complete)).state);
        assert!(track_update(&mut active, progress(2, 5, Canceled)).state);
        assert!(active.is_empty());
    }

    #[test]
    fn sizes_handle_unknown_totals() {
        assert_eq!(sizes(50, 200, 25), (50, Some(200), Some(25)));
        // CEF had no percent yet: derive it.
        assert_eq!(sizes(50, 200, -1), (50, Some(200), Some(25)));
        // No length from the server (0 or -1): no total, no percent.
        assert_eq!(sizes(50, 0, -1), (50, None, None));
        assert_eq!(sizes(50, -1, 0), (50, None, None));
        // Garbage in, clamped out.
        assert_eq!(sizes(-5, 10, 900), (0, Some(10), Some(0)));
        assert_eq!(sizes(20, 10, -1), (20, Some(10), Some(100)));
    }

    #[test]
    fn basetime_converts_to_unix() {
        // 2026-01-01T00:00:00Z.
        assert_eq!(
            basetime_to_unix((1_767_225_600 + 11_644_473_600) * 1_000_000),
            Some(1_767_225_600)
        );
        assert_eq!(basetime_to_unix(0), None);
        assert_eq!(basetime_to_unix(-1), None);
    }

    #[test]
    fn file_name_prefers_saved_path() {
        let mut p = progress(1, 0, DownloadState::InProgress);
        assert_eq!(p.file_name(), "f.zip");
        p.full_path = "/home/u/Downloads/f (1).zip".into();
        assert_eq!(p.file_name(), "f (1).zip");
        p.full_path.clear();
        p.suggested_name = "../../evil/x.sh".into();
        assert_eq!(p.file_name(), "x.sh");
        p.suggested_name.clear();
        assert_eq!(p.file_name(), "download");
    }

    #[test]
    fn unknown_download_cannot_be_controlled() {
        for control in [
            DownloadControl::Cancel,
            DownloadControl::Pause,
            DownloadControl::Resume,
        ] {
            assert!(!control_download(987_654, control));
        }
        assert_eq!(
            PENDING_DOWNLOAD_CHANGES.with_borrow(|c| *c),
            DownloadChanges::default()
        );
    }

    #[test]
    fn state_names_are_stable() {
        use DownloadState::*;
        let names: Vec<_> = [InProgress, Paused, Complete, Canceled, Interrupted]
            .into_iter()
            .map(DownloadState::as_str)
            .collect();
        assert_eq!(
            names,
            [
                "in-progress",
                "paused",
                "complete",
                "canceled",
                "interrupted"
            ]
        );
    }
}
