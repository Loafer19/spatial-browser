// Accept downloads (CEF default cancels), save under ~/Downloads with unique
// names, queue completions for the compositor.
//
// Downloads are never left executable, and executables/scripts/unknown types
// are never handed to xdg-open: the downloads page "Open" reveals their folder
// instead (see `download_open_target`).

use cef::{self, rc::Rc, *};
use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Completed download for downloads.json + desktop notification.
pub struct CompletedDownload {
    pub url: String,
    pub path: String,
}

thread_local! {
    pub static PENDING_DOWNLOADS: RefCell<Vec<CompletedDownload>> = const { RefCell::new(Vec::new()) };
    // on_download_updated repeats after complete — dedupe by download id.
    static NOTIFIED_DOWNLOADS: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
    // Completed this run with a risky server MIME type. Not persisted; after a
    // restart `is_risky_file` still catches them by extension or content.
    static RISKY_BY_MIME: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
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
            _callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(item) = download_item else {
                return;
            };
            if item.is_complete() == 0 {
                return;
            }
            let already_notified =
                NOTIFIED_DOWNLOADS.with_borrow_mut(|seen| !seen.insert(item.id()));
            if already_notified {
                return;
            }
            let path = CefString::from(&item.full_path()).to_string();
            let mime = CefString::from(&item.mime_type()).to_string();
            let path_buf = PathBuf::from(&path);
            strip_exec_bits(&path_buf);
            let name = path_buf.file_name().and_then(|f| f.to_str()).unwrap_or("");
            if is_risky_download(name, Some(&mime)) {
                log::info!("download {path:?} ({mime}) is executable/unknown; Open will reveal its folder");
                let canonical = path_buf.canonicalize().unwrap_or(path_buf);
                RISKY_BY_MIME.with_borrow_mut(|risky| risky.insert(canonical));
            }
            PENDING_DOWNLOADS.with_borrow_mut(|pending| {
                pending.push(CompletedDownload {
                    url: CefString::from(&item.url()).to_string(),
                    path,
                });
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
}
