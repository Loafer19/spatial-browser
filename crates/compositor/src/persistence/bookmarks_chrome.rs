// Import Chromium-family Bookmarks JSON (Chrome / Chromium / Brave / Edge / …)
// into the flat bookmarks.json model. Roots are excluded from folder paths.

use super::bookmarks::Bookmark;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub added: usize,
    pub skipped: usize,
}

#[derive(Deserialize)]
struct ChromeBookmarksFile {
    roots: ChromeRoots,
}

#[derive(Deserialize)]
struct ChromeRoots {
    bookmark_bar: ChromeNode,
    other: ChromeNode,
    synced: ChromeNode,
}

#[derive(Deserialize)]
struct ChromeNode {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    url: Option<String>,
    #[serde(default)]
    children: Vec<ChromeNode>,
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Browser config roots that store a Chromium-style `Bookmarks` JSON file.
fn browser_config_roots() -> Vec<PathBuf> {
    let home = home_dir();
    vec![
        home.join(".config/google-chrome"),
        home.join(".config/google-chrome-beta"),
        home.join(".config/google-chrome-unstable"),
        home.join(".config/chromium"),
        home.join(".config/chromium-browser"),
        home.join(".config/BraveSoftware/Brave-Browser"),
        home.join(".config/BraveSoftware/Brave-Browser-Beta"),
        home.join(".config/microsoft-edge"),
        home.join(".config/microsoft-edge-beta"),
        home.join(".config/vivaldi"),
        home.join(".config/opera"),
        home.join(".config/opera-beta"),
        // Flatpak
        home.join(".var/app/com.google.Chrome/config/google-chrome"),
        home.join(".var/app/com.google.ChromeDev/config/google-chrome"),
        home.join(".var/app/org.chromium.Chromium/config/chromium"),
        home.join(".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser"),
        home.join(".var/app/com.microsoft.Edge/config/microsoft-edge"),
        // Snap (common)
        home.join("snap/chromium/common/chromium"),
        home.join("snap/brave/current/.config/BraveSoftware/Brave-Browser"),
    ]
}

fn mtime(path: &Path) -> SystemTime {
    path.metadata()
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn filesize(path: &Path) -> u64 {
    path.metadata().map(|m| m.len()).unwrap_or(0)
}

/// Names Chrome may use for the bookmarks JSON (local vs account-sync).
const BOOKMARK_FILENAMES: &[&str] = &["Bookmarks", "AccountBookmarks"];

/// Find Chromium-style bookmark JSON under known browser profiles.
/// Prefers larger / newer files (AccountBookmarks often replaces Bookmarks
/// when signed into Chrome sync).
pub fn discover_bookmark_files() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in browser_config_roots() {
        if !root.is_dir() {
            continue;
        }
        // Profile dirs: Default, Profile 1, … directly under the browser root.
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let looks_like_profile = name == "Default"
                || name.starts_with("Profile ")
                || name == "Guest Profile"
                || name == "System Profile";
            if !looks_like_profile {
                continue;
            }
            for fname in BOOKMARK_FILENAMES {
                let bookmarks = path.join(fname);
                // Tiny stubs / empty shells — ignore.
                if bookmarks.is_file() && filesize(&bookmarks) > 64 {
                    found.push(bookmarks);
                }
            }
        }
    }
    found.sort_by(|a, b| {
        filesize(b)
            .cmp(&filesize(a))
            .then_with(|| mtime(b).cmp(&mtime(a)))
    });
    found
}

/// Best starting directory / file hint for the native file dialog.
pub fn dialog_start() -> (Option<PathBuf>, Option<String>) {
    let found = discover_bookmark_files();
    if let Some(best) = found.first() {
        let name = best
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Bookmarks".into());
        return (best.parent().map(|p| p.to_path_buf()), Some(name));
    }
    // Nothing found — open Default under the first existing browser root.
    for root in browser_config_roots() {
        let default = root.join("Default");
        if default.is_dir() {
            return (Some(default), None);
        }
        if root.is_dir() {
            return (Some(root), None);
        }
    }
    let config = home_dir().join(".config");
    if config.is_dir() {
        return (Some(config), None);
    }
    (Some(home_dir()), None)
}

pub fn expand_user_path(path: &str) -> PathBuf {
    let path = path.trim();
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

/// Parse Chrome Bookmarks file bytes into flat bookmarks (roots not in folder path).
pub fn parse_chrome_bookmarks(bytes: &[u8]) -> Result<Vec<Bookmark>, String> {
    let chrome: ChromeBookmarksFile =
        serde_json::from_slice(bytes).map_err(|e| format!("not a Chromium Bookmarks file: {e}"))?;
    let mut imported = Vec::new();
    for root in [
        &chrome.roots.bookmark_bar,
        &chrome.roots.other,
        &chrome.roots.synced,
    ] {
        for child in &root.children {
            collect(child, "", &mut imported);
        }
    }
    Ok(imported)
}

fn collect(node: &ChromeNode, parent_path: &str, out: &mut Vec<Bookmark>) {
    match node.kind.as_str() {
        "url" => {
            if let Some(url) = &node.url {
                out.push(Bookmark {
                    url: url.clone(),
                    title: Some(node.name.clone()),
                    folder: if parent_path.is_empty() {
                        None
                    } else {
                        Some(parent_path.to_string())
                    },
                });
            }
        }
        "folder" => {
            let child_path = if parent_path.is_empty() {
                node.name.clone()
            } else {
                format!("{}/{}", parent_path, node.name)
            };
            for child in &node.children {
                collect(child, &child_path, out);
            }
        }
        _ => {}
    }
}

/// Merge Chromium bookmarks from `path` into `bookmarks` (dedupe by URL).
pub fn import_path(bookmarks: &mut Vec<Bookmark>, path: &str) -> Result<ImportStats, String> {
    let path = expand_user_path(path);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let imported = parse_chrome_bookmarks(&bytes)?;
    if imported.is_empty() {
        return Err("no bookmarks found in file".into());
    }
    let existing: HashSet<String> = bookmarks.iter().map(|b| b.url.clone()).collect();
    let mut stats = ImportStats::default();
    for b in imported {
        if existing.contains(&b.url) {
            stats.skipped += 1;
        } else {
            bookmarks.push(b);
            stats.added += 1;
        }
    }
    Ok(stats)
}
