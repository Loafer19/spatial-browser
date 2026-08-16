// Classic URL bookmarks — a separate file from session.json (persistence.rs)
// on purpose: bookmarks change rarely and deliberately (one hotkey press),
// unlike canvas state which changes continuously, and should survive
// independent of whatever happens to the canvas session.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub url: String,
    // `None` until renamed — CEF only exposes document title via an
    // async callback we don't wire up, so `display_label` falls back to
    // the URL's host until the user sets one explicitly.
    #[serde(default)]
    pub title: Option<String>,
    // `None` = ungrouped. Set via the same rename form as `title`.
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct BookmarksFile {
    bookmarks: Vec<Bookmark>,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/bookmarks.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<Bookmark> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BookmarksFile>(&bytes).ok())
        .map(|f| f.bookmarks)
        .unwrap_or_default()
}

pub fn save(bookmarks: &[Bookmark]) {
    let data = BookmarksFile {
        bookmarks: bookmarks.to_vec(),
    };
    let path = path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("bookmarks save: couldn't create {parent:?}: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(&data) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("bookmarks save: couldn't write {path:?}: {e}");
            }
        }
        Err(e) => log::warn!("bookmarks save: serialize failed: {e}"),
    }
}

/// Bare host from a URL — the fallback display label, and used to guess
/// a favicon location (`https://{host}/favicon.ico`).
pub fn host_of(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme)
}

/// What to show for a bookmark: its title if renamed, else its host.
pub fn display_label(bookmark: &Bookmark) -> &str {
    bookmark
        .title
        .as_deref()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| host_of(&bookmark.url))
}
