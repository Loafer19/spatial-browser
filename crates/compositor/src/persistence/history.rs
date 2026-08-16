// Persisted omnibox input history — what was actually *typed* into the
// new-page prompt (pages::omnibox), not the URL it resolved to. Its own
// history.json, separate from bookmarks.json/the canvas session:
// different concern, different shape (a capped, deduped list of raw
// strings), and it should survive independent of either.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// How many entries to keep, and to show as quick-repeat chips on the
// omnibox page.
const MAX_HISTORY: usize = 50;

#[derive(Default, Serialize, Deserialize)]
struct HistoryFile {
    entries: Vec<String>,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/history.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<String> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<HistoryFile>(&bytes).ok())
        .map(|f| f.entries)
        .unwrap_or_default()
}

/// Records `raw` as the most recent entry (moving it to the front if it
/// was already present, rather than duplicating it) and saves
/// immediately — history entries are deliberate, infrequent actions
/// (one omnibox submission), not something that needs debouncing.
pub fn record(history: &mut Vec<String>, raw: &str) {
    history.retain(|entry| entry != raw);
    history.insert(0, raw.to_string());
    history.truncate(MAX_HISTORY);
    save(history);
}

fn save(history: &[String]) {
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
