// history.json — completed top-level visits (not omnibox typed_history).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_HISTORY: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    // Unix seconds UTC; local display/grouping is client-side JS.
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

/// Record visit at front; refresh timestamp if same URL is already top.
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
