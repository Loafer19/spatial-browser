// typed_history.json — raw omnibox input (not resolved URLs / visit log).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_TYPED_HISTORY: usize = 50;

#[derive(Default, Serialize, Deserialize)]
struct TypedHistoryFile {
    entries: Vec<String>,
}

fn path() -> PathBuf {
    crate::persistence::config_path("typed_history.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<String> {
    super::store::load_json::<TypedHistoryFile>(&path(), "typed history")
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
    super::store::save_json_bg(&path(), "typed history", &data, super::store::Backup::None);
}
