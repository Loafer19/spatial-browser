// User-editable preferences: ad-block on/off, the omnibox's default
// search engine, and hosts the user has added to the ad/tracker
// blocklist themselves (on top of cef-bridge's compiled-in list —
// see blocklist.rs's own header comment for why that one's static
// instead of user-editable). One object, not a list, so unlike
// bookmarks/history/downloads/workspaces this is a single JSON object
// rather than a `{ entries: [...] }` wrapper.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// `#[serde(default)]` at the container level: a field added here in the
// future, after real settings.json files already exist on disk without
// it, falls back to its own default instead of failing to parse the
// whole file just because one key is missing.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub ad_block_enabled: bool,
    /// Strips utm_*/fbclid/gclid/etc. tracking params from a link
    /// before it loads — see cef-bridge::clean_urls.
    pub clean_urls_enabled: bool,
    /// A full search URL template with the query appended directly
    /// (e.g. `https://www.google.com/search?q=`) — the same shape
    /// omnibox.rs's `@prefix` engines already use, so applying it is
    /// just using this instead of that map's hardcoded default.
    pub default_search_engine: String,
    /// Index into `reader_mode::READER_THEMES` — the colors Ctrl+Shift+R
    /// (reader mode) rewrites a page's content with. Independent of the
    /// UI chrome's own `Theme`/`THEMES`: reading comfort (light/sepia/
    /// dark article background) is a different concern than the
    /// canvas/list-page chrome color scheme.
    pub reader_theme: usize,
    pub custom_blocked_hosts: Vec<String>,
    /// CEF's `windowless_frame_rate` for every page, and the main
    /// event loop's own pacing (main.rs) — one of 60/90/120, picked
    /// from Settings, not free text (a monitor's actual max refresh
    /// rate is the real ceiling regardless of this; higher just gives
    /// CEF/the loop room to produce frames that fast if the display can
    /// show them).
    pub target_fps: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ad_block_enabled: true,
            clean_urls_enabled: true,
            default_search_engine: "https://www.google.com/search?q=".to_string(),
            reader_theme: 0,
            custom_blocked_hosts: Vec::new(),
            target_fps: 60,
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
