// Persisted download history — what got saved to disk and where, for
// the Ctrl+J downloads-list page. Its own downloads.json, separate
// from bookmarks.json/history.json: a download record isn't a bookmark
// (it's a file, not a page) and isn't typed input (it's something CEF's
// DownloadHandler told cef-bridge about, not the user).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Caps the file's growth — old entries fall off the end. Downloads are
// frequent enough (unlike bookmarks) that an unbounded list would grow
// forever.
const MAX_DOWNLOADS: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub url: String,
    pub path: String,
}

#[derive(Default, Serialize, Deserialize)]
struct DownloadsFile {
    entries: Vec<DownloadRecord>,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/downloads.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<DownloadRecord> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<DownloadsFile>(&bytes).ok())
        .map(|f| f.entries)
        .unwrap_or_default()
}

/// Records a newly completed download as the most recent entry and
/// saves immediately — downloads finish rarely enough (relative to,
/// say, a canvas drag) that there's no need for the debounce the
/// canvas session itself uses.
pub fn record(downloads: &mut Vec<DownloadRecord>, entry: DownloadRecord) {
    downloads.insert(0, entry);
    downloads.truncate(MAX_DOWNLOADS);
    save(downloads);
}

pub fn save(downloads: &[DownloadRecord]) {
    let data = DownloadsFile {
        entries: downloads.to_vec(),
    };
    let path = path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("downloads save: couldn't create {parent:?}: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(&data) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("downloads save: couldn't write {path:?}: {e}");
            }
        }
        Err(e) => log::warn!("downloads save: serialize failed: {e}"),
    }
}
