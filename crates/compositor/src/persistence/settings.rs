// User-editable preferences: ad-block on/off, the omnibox's default
// search engine, and hosts the user has added to the ad/tracker
// blocklist themselves (on top of cef-bridge's compiled-in list —
// see blocklist.rs's own header comment for why that one's static
// instead of user-editable). One object, not a list, so unlike
// bookmarks/history/downloads/workspaces this is a single JSON object
// rather than a `{ entries: [...] }` wrapper.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub ad_block_enabled: bool,
    /// A full search URL template with the query appended directly
    /// (e.g. `https://www.google.com/search?q=`) — the same shape
    /// omnibox.rs's `@prefix` engines already use, so applying it is
    /// just using this instead of that map's hardcoded default.
    pub default_search_engine: String,
    pub custom_blocked_hosts: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ad_block_enabled: true,
            default_search_engine: "https://www.google.com/search?q=".to_string(),
            custom_blocked_hosts: Vec::new(),
        }
    }
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/settings.json")
}

/// Falls back to `AppSettings::default()` if there's no file yet or it
/// fails to parse.
pub fn load() -> AppSettings {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Settings edits are rare, deliberate actions — saved immediately, no
/// debounce.
pub fn save(settings: &AppSettings) {
    let path = path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("settings save: couldn't create {parent:?}: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(settings) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("settings save: couldn't write {path:?}: {e}");
            }
        }
        Err(e) => log::warn!("settings save: serialize failed: {e}"),
    }
}
