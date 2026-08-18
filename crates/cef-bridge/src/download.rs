// Downloads: accept every one (the default is to cancel — see the
// comments below), save under ~/Downloads with a collision-safe name,
// no native save-as dialog, and queue completions for the compositor
// to record + notify on.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One completed download, queued for the compositor to record into
/// downloads.json and surface as a desktop notification.
pub struct CompletedDownload {
    pub url: String,
    pub path: String,
}

thread_local! {
    // Appended by OsrDownloadHandler::on_download_updated the first time
    // a given download reports complete; drained once per frame by the
    // compositor's redraw handler. A queue, not a single slot like
    // PENDING_BOOKMARK/PENDING_OMNIBOX/PENDING_SWITCH: downloads finish
    // on their own schedule rather than in response to one user gesture,
    // and more than one can complete inside the same frame.
    pub static PENDING_DOWNLOADS: RefCell<Vec<CompletedDownload>> = const { RefCell::new(Vec::new()) };
    // CEF calls on_download_updated repeatedly, including after a
    // download is already complete — this tracks which download ids
    // already got queued into PENDING_DOWNLOADS so they don't get
    // queued (and so double-notified/double-logged) again.
    static NOTIFIED_DOWNLOADS: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
}

fn downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    let dir = PathBuf::from(home).join("Downloads");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Picks a non-colliding path for `suggested_name` inside `dir` —
/// `name.pdf`, then `name (1).pdf`, `name (2).pdf`, ... — so a second
/// download of the same filename never silently overwrites the first.
fn unique_download_path(dir: &Path, suggested_name: &str) -> PathBuf {
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
        // Default (unimplemented) behavior cancels every download — this
        // is the "yes, allow it" half of that default.
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
            PENDING_DOWNLOADS.with_borrow_mut(|pending| {
                pending.push(CompletedDownload {
                    url: CefString::from(&item.url()).to_string(),
                    path: CefString::from(&item.full_path()).to_string(),
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
