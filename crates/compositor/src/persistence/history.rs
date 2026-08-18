// Persisted browsing history — actual page visits (cef-bridge's
// OsrLoadHandler::on_load_end, one entry per completed top-level
// navigation), not what was typed into the omnibox (typed_history.rs)
// and not bookmarks. Its own history.json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Caps the file's growth — old entries fall off the end. Real visits
// happen far more often than deliberate bookmark edits, so an
// unbounded list would grow forever.
const MAX_HISTORY: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    // Unix seconds, UTC — display formatting (local time, grouping by
    // day) happens client-side in pages::history_list's JS, not here.
    pub visited_at: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct HistoryFile {
    entries: Vec<HistoryEntry>,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/history.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<HistoryEntry> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<HistoryFile>(&bytes).ok())
        .map(|f| f.entries)
        .unwrap_or_default()
}

/// Records a page visit as the most recent entry and saves
/// immediately. If the same URL is already the most recent entry (a
/// refresh, or a SPA's repeated soft-navigation to the same URL) its
/// timestamp is just refreshed in place instead of spamming the list
/// with duplicates.
pub fn record(history: &mut Vec<HistoryEntry>, url: &str, visited_at: i64) {
    match history.first_mut() {
        Some(top) if top.url == url => top.visited_at = visited_at,
        _ => history.insert(
            0,
            HistoryEntry {
                url: url.to_string(),
                visited_at,
            },
        ),
    }
    history.truncate(MAX_HISTORY);
    save(history);
}

pub fn save(history: &[HistoryEntry]) {
    let data = HistoryFile {
        entries: history.to_vec(),
    };
    let path = path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("history save: couldn't create {parent:?}: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(&data) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("history save: couldn't write {path:?}: {e}");
            }
        }
        Err(e) => log::warn!("history save: serialize failed: {e}"),
    }
}
