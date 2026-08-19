// One-time migration: read Chrome's Bookmarks JSON and merge the entries
// into spatial-browser's own bookmarks.json, preserving existing bookmarks.
//
// Chrome stores bookmarks as a tree of arbitrary depth, under three fixed
// roots ("Bookmarks bar", "Other bookmarks", "Mobile bookmarks"/"synced").
// This tool flattens that tree into the existing flat `folder: Option<String>`
// scheme by joining ancestor folder names with `/`, *excluding* whichever of
// the three roots a bookmark happens to live under — that's Chrome's own
// top-level organization, not a folder the user actually created, and
// including it put a literal "Bookmarks bar/" (or worse, "Bookmarks bar"
// with nothing else) on every single imported bookmark:
//
//   Bookmarks bar / Work / Project  →  folder = "Work/Project"
//   Bookmarks bar / Work            →  folder = "Work"
//   Bookmarks bar (no subfolder)    →  folder = None
//
// This means the existing bookmarks.json format and all compositor code that
// reads it are unchanged — no schema migration, no UI changes needed.
//
// Usage:
//   import-chrome
//       Reads  ~/.config/google-chrome/Default/Bookmarks
//       Writes ~/.config/spatial-browser/bookmarks.json
//
//   import-chrome /path/to/chrome/profile/Bookmarks
//       Explicit source path.
//
//   import-chrome /path/to/Bookmarks /path/to/bookmarks.json
//       Explicit source and destination paths.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

// ── Chrome on-disk format ────────────────────────────────────────────────────

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
    // Present when kind == "url"
    url: Option<String>,
    // Present when kind == "folder"
    #[serde(default)]
    children: Vec<ChromeNode>,
}

// ── spatial-browser on-disk format (mirrors persistence/bookmarks.rs) ────────

#[derive(Clone, Serialize, Deserialize)]
struct Bookmark {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    folder: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct BookmarksFile {
    bookmarks: Vec<Bookmark>,
}

// ── Conversion ───────────────────────────────────────────────────────────────

/// Walk a Chrome node tree, accumulating flat `Bookmark`s.
///
/// `parent_path` is the `/`-joined folder path of all ancestors above this
/// node. It is empty for the root nodes so that top-level bookmarks end up
/// with `folder: None` (ungrouped), matching what the user would get if they
/// added those bookmarks manually.
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
            // Build the path component for this folder's children.
            let child_path = if parent_path.is_empty() {
                node.name.clone()
            } else {
                format!("{}/{}", parent_path, node.name)
            };
            for child in &node.children {
                collect(child, &child_path, out);
            }
        }
        // Unknown node types (e.g. future Chrome additions) — skip silently.
        _ => {}
    }
}

// ── Paths ────────────────────────────────────────────────────────────────────

fn default_chrome_path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/google-chrome/Default/Bookmarks")
}

fn default_output_path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/bookmarks.json")
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let chrome_path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_chrome_path);
    let output_path = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(default_output_path);

    // Read Chrome bookmarks.
    let chrome_bytes = std::fs::read(&chrome_path)
        .with_context(|| format!("reading {}", chrome_path.display()))?;
    let chrome: ChromeBookmarksFile = serde_json::from_slice(&chrome_bytes)
        .with_context(|| format!("parsing {}", chrome_path.display()))?;

    // Collect all bookmarks from all three roots — starting each one's
    // *children* at an empty path rather than calling `collect` on the root
    // itself, so none of the three roots' own names ("Bookmarks bar", etc.)
    // end up as part of any folder path (see this file's header comment).
    let mut imported: Vec<Bookmark> = Vec::new();
    for root in [
        &chrome.roots.bookmark_bar,
        &chrome.roots.other,
        &chrome.roots.synced,
    ] {
        for child in &root.children {
            collect(child, "", &mut imported);
        }
    }

    eprintln!(
        "import-chrome: found {} bookmark(s) in {}",
        imported.len(),
        chrome_path.display()
    );

    // Load existing bookmarks from spatial-browser (or start empty).
    let existing: BookmarksFile = std::fs::read(&output_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    // Merge: keep existing entries, append imported ones not already present
    // (dedup by URL).
    let existing_urls: HashSet<&str> = existing.bookmarks.iter().map(|b| b.url.as_str()).collect();
    let imported_count = imported.len();
    let new_entries: Vec<Bookmark> = imported
        .into_iter()
        .filter(|b| !existing_urls.contains(b.url.as_str()))
        .collect();

    eprintln!(
        "import-chrome: {} new (skipping {} already present)",
        new_entries.len(),
        imported_count - new_entries.len(),
    );

    let mut merged = existing.bookmarks;
    merged.extend(new_entries);

    let result = BookmarksFile { bookmarks: merged };

    // Write output, creating directories as needed.
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&result).context("serializing bookmarks")?;
    std::fs::write(&output_path, bytes)
        .with_context(|| format!("writing {}", output_path.display()))?;

    eprintln!("import-chrome: wrote {}", output_path.display());
    Ok(())
}
