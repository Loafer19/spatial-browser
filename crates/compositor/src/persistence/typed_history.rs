// typed_history.json — raw omnibox input (not resolved URLs / visit log).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_TYPED_HISTORY: usize = 50;

#[derive(Default, Serialize, Deserialize)]
struct TypedHistoryFile {
    entries: Vec<String>,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/typed_history.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<String> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<TypedHistoryFile>(&bytes).ok())
        .map(|f| f.entries)
        .unwrap_or_default()
}

/// Move `raw` to front (dedupe) and save immediately.
pub fn record(typed_history: &mut Vec<String>, raw: &str) {
    typed_history.retain(|entry| entry != raw);
    typed_history.insert(0, raw.to_string());
    typed_history.truncate(MAX_TYPED_HISTORY);
    save(typed_history);
}

fn save(typed_history: &[String]) {
    let data = TypedHistoryFile {
        entries: typed_history.to_vec(),
    };
    let path = path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("typed history save: couldn't create {parent:?}: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(&data) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("typed history save: couldn't write {path:?}: {e}");
            }
        }
        Err(e) => log::warn!("typed history save: serialize failed: {e}"),
    }
}
