// Stylus-style user CSS — same inject path as userscripts, but `.css`
// files under `~/.config/spatial-browser/userstyles/`.
//
// Metadata (any of these forms, one tag per line):
//
//   /* @name   Dim bookmarks */
//   /* @match  spatial-ui */
//   /* @match  *://*.example.com/* */
//   /* @exclude *://*.example.com/admin/* */
//
// Special match tokens for built-in `data:` UI pages (bookmarks, settings,
// help, omnibox, …) — those URLs are huge `data:text/html…` blobs, so a
// normal site glob never hits them:
//
//   spatial-ui   — any ephemeral chrome page
//   spatial:*    — same
//
// A style with no `@match` is skipped. Disabled basenames live in
// `userstyles_state.json`. Ctrl+Shift+U lists scripts and styles together.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct UserStyle {
    pub file_name: String,
    pub name: String,
    pub matches: Vec<String>,
    pub excludes: Vec<String>,
    pub css: String,
    pub enabled: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct StateFile {
    disabled: HashSet<String>,
}

fn styles_dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/userstyles")
}

fn state_path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/userstyles_state.json")
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

pub fn load() -> Vec<UserStyle> {
    let state = load_state();
    let Ok(entries) = std::fs::read_dir(styles_dir()) else {
        return Vec::new();
    };
    let mut styles = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("css") {
            continue;
        }
        if let Some(style) = parse_file(&path, &state.disabled) {
            styles.push(style);
        }
    }
    styles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    styles
}

pub fn reload(styles: &mut Vec<UserStyle>) {
    *styles = load();
    log::info!("userstyles: reloaded {} style(s)", styles.len());
}

pub fn toggle_enabled(styles: &mut [UserStyle], file_name: &str) -> bool {
    let Some(style) = styles.iter_mut().find(|s| s.file_name == file_name) else {
        return false;
    };
    style.enabled = !style.enabled;
    let mut state = load_state();
    if style.enabled {
        state.disabled.remove(file_name);
    } else {
        state.disabled.insert(file_name.to_string());
    }
    save_state(&state);
    true
}

pub fn open_dir() {
    let dir = styles_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = std::process::Command::new("xdg-open").arg(&dir).spawn() {
        log::warn!("xdg-open userstyles dir failed: {e}");
    }
}

fn parse_meta_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    // /* @name value */  or  /* @name value
    let rest = line.strip_prefix("/*")?.trim_start();
    let rest = rest.strip_prefix('@')?;
    let (tag, value) = rest.split_once(char::is_whitespace)?;
    let value = value
        .trim()
        .trim_end_matches("*/")
        .trim()
        .trim_start_matches(':')
        .trim();
    if value.is_empty() {
        return None;
    }
    Some((tag, value))
}

fn parse_file(path: &Path, disabled: &HashSet<String>) -> Option<UserStyle> {
    let file_name = path.file_name()?.to_str()?.to_string();
    let Ok(css) = std::fs::read_to_string(path) else {
        log::warn!("userstyles: couldn't read {path:?}");
        return None;
    };
    let mut name = file_name.trim_end_matches(".css").to_string();
    let mut matches = Vec::new();
    let mut excludes = Vec::new();
    for line in css.lines() {
        let Some((tag, value)) = parse_meta_line(line) else {
            continue;
        };
        match tag {
            "name" => name = value.to_string(),
            "match" => matches.push(value.to_string()),
            "exclude" => excludes.push(value.to_string()),
            _ => {}
        }
    }
    if matches.is_empty() {
        log::warn!("userstyles: {path:?} has no `/* @match */` lines, skipping");
        return None;
    }
    Some(UserStyle {
        enabled: !disabled.contains(&file_name),
        file_name,
        name,
        matches,
        excludes,
        css,
    })
}

fn is_spatial_ui_token(pattern: &str) -> bool {
    matches!(pattern, "spatial-ui" | "spatial:*" | "spatial://*" | "spatial://ui")
}

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

fn style_applies(url: &str, ephemeral: bool, style: &UserStyle) -> bool {
    if !style.enabled {
        return false;
    }
    if !style
        .matches
        .iter()
        .any(|p| matches_pattern(p, url, ephemeral))
    {
        return false;
    }
    !style
        .excludes
        .iter()
        .any(|p| matches_pattern(p, url, ephemeral))
}

/// JS snippets that inject matching stylesheets into the page.
pub fn matching_inject_js(url: &str, ephemeral: bool, styles: &[UserStyle]) -> Vec<String> {
    styles
        .iter()
        .filter(|s| style_applies(url, ephemeral, s))
        .map(|s| {
            let css_json = serde_json::to_string(&s.css).unwrap_or_else(|_| "\"\"".into());
            let id_json = serde_json::to_string(&s.file_name).unwrap_or_else(|_| "\"\"".into());
            format!(
                "(function(){{\
                   var id={id_json};\
                   if(document.querySelector('style[data-spatial-userstyle=\"'+id+'\"]')) return;\
                   var s=document.createElement('style');\
                   s.setAttribute('data-spatial-userstyle', id);\
                   s.textContent={css_json};\
                   (document.documentElement||document.head||document.body).appendChild(s);\
                 }})();"
            )
        })
        .collect()
}
