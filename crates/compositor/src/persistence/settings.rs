// User-editable preferences. One JSON object in settings.json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Which built-in host/filter subscriptions are enabled.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterLists {
    /// Compiled-in Peter Lowe ad-server hosts (`blocked_domains.txt`).
    pub peter_lowe: bool,
    /// EasyList (network + cosmetic) — wired in a later phase.
    pub easylist: bool,
    /// EasyPrivacy — wired in a later phase.
    pub easyprivacy: bool,
}

impl Default for FilterLists {
    fn default() -> Self {
        Self {
            peter_lowe: true,
            easylist: true,
            easyprivacy: true,
        }
    }
}

// `#[serde(default)]` at the container level: a field added later falls
// back instead of failing to parse older settings.json files.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Master content-filtering switch. Off → no list / layer applies.
    pub ad_block_enabled: bool,
    /// Apply network (request-cancel) rules from enabled lists.
    pub filter_network_enabled: bool,
    /// Apply cosmetic hiding (CSS inject) — no-op until P2.
    pub filter_cosmetic_enabled: bool,
    /// Apply scriptlets (`+js`) — no-op until P3; default Off.
    pub filter_scriptlets_enabled: bool,
    pub filter_lists: FilterLists,
    /// Strips utm_*/fbclid/gclid/etc. from navigations.
    pub clean_urls_enabled: bool,
    pub default_search_engine: String,
    pub reader_theme: usize,
    pub custom_blocked_hosts: Vec<String>,
    pub target_fps: u32,
    /// Last Settings UI tab: `general` | `blocking` | `appearance`.
    pub settings_tab: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ad_block_enabled: true,
            filter_network_enabled: true,
            filter_cosmetic_enabled: true,
            filter_scriptlets_enabled: false,
            filter_lists: FilterLists::default(),
            clean_urls_enabled: true,
            default_search_engine: "https://www.google.com/search?q=".to_string(),
            reader_theme: 0,
            custom_blocked_hosts: Vec::new(),
            target_fps: 60,
            settings_tab: "general".to_string(),
        }
    }
}

impl AppSettings {
    pub fn normalize_tab(tab: &str) -> &'static str {
        match tab {
            "blocking" => "blocking",
            "appearance" => "appearance",
            _ => "general",
        }
    }
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/settings.json")
}

pub fn load() -> AppSettings {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

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
