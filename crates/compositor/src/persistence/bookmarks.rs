// bookmarks.json — deliberate edits, separate from the live canvas session.

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
    crate::persistence::config_path("bookmarks.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<Bookmark> {
    super::store::load_json::<BookmarksFile>(&path(), "bookmarks")
        .map(|f| f.bookmarks)
        .unwrap_or_default()
}

pub fn save(bookmarks: &[Bookmark]) {
    let data = BookmarksFile {
        bookmarks: bookmarks.to_vec(),
    };
    super::store::save_json(&path(), "bookmarks", &data, super::store::Backup::Keep);
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
