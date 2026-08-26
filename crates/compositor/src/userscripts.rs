// A Tampermonkey/Greasemonkey-style userscript runner — not real Chrome
// extensions (CEF's Alloy/windowless bootstrap exposes no extension-
// loading API at all, confirmed empirically: zero of ~5900 bound
// functions touch it), just the one slice of what extensions are
// commonly used for that's already exactly what this codebase's own
// clipboard_bridge.rs/blocklist.rs do by hand: inject JS into pages
// whose URL matches a pattern.
//
// Each `.js` file under `~/.config/spatial-browser/userscripts/` is one
// script. Metadata lines anywhere in the file (Tampermonkey-style):
//
//   // @name   My script
//   // @match  *://*.example.com/*
//   // @exclude *://*.example.com/admin/*
//   // @run-at document-end
//
// `@match` / `@exclude` are plain wildcard globs against the full URL
// (`*` = anything), plus the special tokens `spatial-ui` / `spatial:*`
// for built-in ephemeral chrome pages (bookmarks, settings, …) — same
// as userstyles. `@run-at` is `document-start`, `document-end`
// (default), or `document-idle`. A script with no `@match` is skipped.
// Disabled filenames are tracked in `userscripts_state.json` next to
// the scripts dir — toggling in the Ctrl+Shift+U list flips that, not
// the file on disk.
//
// A small GM_* prelude is prepended on inject (`GM_addStyle`,
// `GM_getValue`, `GM_setValue` backed by localStorage) so common
// GreasyFork scripts work without a full Violentmonkey API.
//
// `reload()` re-reads the directory (and state file) without restarting
// the browser — the userscripts list page's Reload button and the
// Ctrl+Shift+U open-or-reload path both call it.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunAt {
    DocumentStart,
    DocumentEnd,
    DocumentIdle,
}

impl RunAt {
    fn parse(s: &str) -> Self {
        match s.trim() {
            "document-start" => Self::DocumentStart,
            "document-idle" => Self::DocumentIdle,
            _ => Self::DocumentEnd,
        }
    }
}

#[derive(Clone)]
pub struct UserScript {
    pub file_name: String,
    pub name: String,
    pub matches: Vec<String>,
    pub excludes: Vec<String>,
    pub run_at: RunAt,
    pub code: String,
    pub enabled: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct StateFile {
    /// Basenames of scripts the user turned off in the list UI.
    disabled: HashSet<String>,
}

fn scripts_dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/userscripts")
}

fn state_path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/userscripts_state.json")
}

fn load_state() -> StateFile {
    let Ok(text) = std::fs::read_to_string(state_path()) else {
        return StateFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_state(state: &StateFile) {
    if let Ok(text) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(state_path(), text);
    }
}

/// Reads every `.js` file in the userscripts directory. Missing
/// directory or unreadable/matchless files are skipped, not errors.
pub fn load() -> Vec<UserScript> {
    let state = load_state();
    let Ok(entries) = std::fs::read_dir(scripts_dir()) else {
        return Vec::new();
    };
    let mut scripts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("js") {
            continue;
        }
        if let Some(script) = parse_file(&path, &state.disabled) {
            scripts.push(script);
        }
    }
    scripts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    scripts
}

/// Re-read scripts + state from disk into `scripts` in place.
pub fn reload(scripts: &mut Vec<UserScript>) {
    *scripts = load();
    log::info!("userscripts: reloaded {} script(s)", scripts.len());
}

/// Flip enabled for the script with this basename; persists to state file.
pub fn toggle_enabled(scripts: &mut [UserScript], file_name: &str) -> bool {
    let Some(script) = scripts.iter_mut().find(|s| s.file_name == file_name) else {
        return false;
    };
    script.enabled = !script.enabled;
    let mut state = load_state();
    if script.enabled {
        state.disabled.remove(file_name);
    } else {
        state.disabled.insert(file_name.to_string());
    }
    save_state(&state);
    true
}

pub fn open_dir() {
    let dir = scripts_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = std::process::Command::new("xdg-open").arg(&dir).spawn() {
        log::warn!("xdg-open userscripts dir failed: {e}");
    }
}

fn parse_file(path: &Path, disabled: &HashSet<String>) -> Option<UserScript> {
    let file_name = path.file_name()?.to_str()?.to_string();
    let Ok(code) = std::fs::read_to_string(path) else {
        log::warn!("userscripts: couldn't read {path:?}");
        return None;
    };
    let mut name = file_name.trim_end_matches(".js").to_string();
    let mut matches = Vec::new();
    let mut excludes = Vec::new();
    let mut run_at = RunAt::DocumentEnd;
    for line in code.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("// @name") {
            let n = rest.trim().trim_start_matches(':').trim();
            if !n.is_empty() {
                name = n.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("// @match") {
            let p = rest.trim().trim_start_matches(':').trim();
            if !p.is_empty() {
                matches.push(p.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("// @exclude") {
            let p = rest.trim().trim_start_matches(':').trim();
            if !p.is_empty() {
                excludes.push(p.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("// @run-at") {
            run_at = RunAt::parse(rest.trim().trim_start_matches(':').trim());
        }
    }
    if matches.is_empty() {
        log::warn!("userscripts: {path:?} has no `// @match` lines, skipping");
        return None;
    }
    Some(UserScript {
        enabled: !disabled.contains(&file_name),
        file_name,
        name,
        matches,
        excludes,
        run_at,
        code,
    })
}

fn is_spatial_ui_token(pattern: &str) -> bool {
    matches!(
        pattern,
        "spatial-ui" | "spatial:*" | "spatial://*" | "spatial://ui"
    )
}

/// Wildcard glob (`*` = anything) against the full URL, with the
/// `*.example.com` apex-domain special case (see header), plus
/// `spatial-ui` / `spatial:*` for ephemeral built-in pages.
fn matches_pattern(pattern: &str, url: &str, ephemeral: bool) -> bool {
    if is_spatial_ui_token(pattern) {
        return ephemeral;
    }
    if glob_match(pattern, url) {
        return true;
    }
    if pattern.contains("*.") {
        return glob_match(&pattern.replace("*.", ""), url);
    }
    false
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !text[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if i == parts.len() - 1 {
            return text[pos..].ends_with(part);
        } else {
            match text[pos..].find(part) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
    }
    true
}

fn url_matches_script(url: &str, ephemeral: bool, script: &UserScript) -> bool {
    if !script.enabled {
        return false;
    }
    if !script
        .matches
        .iter()
        .any(|pattern| matches_pattern(pattern, url, ephemeral))
    {
        return false;
    }
    !script
        .excludes
        .iter()
        .any(|pattern| matches_pattern(pattern, url, ephemeral))
}

/// GM_* prelude prepended so GreasyFork-style scripts have a minimal API.
const GM_PRELUDE: &str = r#"(function(){
  if (window.__spatialGM) return;
  window.__spatialGM = true;
  window.GM_addStyle = function(css) {
    var s = document.createElement('style');
    s.textContent = css;
    (document.head || document.documentElement).appendChild(s);
  };
  window.GM_getValue = function(key, def) {
    try {
      var v = localStorage.getItem('gm:' + key);
      return v === null ? def : JSON.parse(v);
    } catch (e) { return def; }
  };
  window.GM_setValue = function(key, val) {
    try { localStorage.setItem('gm:' + key, JSON.stringify(val)); } catch (e) {}
  };
})();"#;

/// Code to inject for `url` at the given run-at timing, each entry
/// already wrapped with the GM prelude. `ephemeral` enables `@match
/// spatial-ui` (built-in chrome pages).
pub fn matching_code(
    url: &str,
    ephemeral: bool,
    scripts: &[UserScript],
    run_at: RunAt,
) -> Vec<String> {
    scripts
        .iter()
        .filter(|s| s.run_at == run_at && url_matches_script(url, ephemeral, s))
        .map(|s| {
            if run_at == RunAt::DocumentIdle {
                format!(
                    "{GM_PRELUDE}\n(function(){{\n  var run=function(){{\n{}\n  }};\n  if(document.readyState==='complete') run();\n  else window.addEventListener('load', run);\n}})();",
                    s.code
                )
            } else {
                format!("{GM_PRELUDE}\n{}", s.code)
            }
        })
        .collect()
}


