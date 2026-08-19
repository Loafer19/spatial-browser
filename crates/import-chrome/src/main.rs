// One-time migration: read Chrome's Bookmarks JSON and History SQLite file and
// merge them into spatial-browser's own bookmarks.json / history.json,
// preserving any existing entries.
//
// Chrome stores bookmarks as a tree of arbitrary depth. This tool flattens
// that tree into the existing flat `folder: Option<String>` scheme by joining
// ancestor folder names with `/`:
//
//   Bookmarks bar / Work / Project  →  folder = "Bookmarks bar/Work/Project"
//   Bookmarks bar / Work            →  folder = "Bookmarks bar/Work"
//   (root level with no parents)    →  folder = None
//
// History: Chrome's History file is an SQLite database. The `urls` table
// contains the URL and title, the `visits` table contains per-visit timestamps
// (stored as microseconds since 1601-01-01 00:00:00 UTC, the Windows FILETIME
// epoch). This tool converts those timestamps to Unix seconds (UTC) and merges
// them into spatial-browser's history.json, keeping existing entries and
// skipping duplicates (same URL + visited_at pair).
//
// Usage:
//   import-chrome
//       Reads  ~/.config/google-chrome/Default/Bookmarks
//              ~/.config/google-chrome/Default/History
//       Writes ~/.config/spatial-browser/bookmarks.json
//              ~/.config/spatial-browser/history.json
//
//   import-chrome /path/to/chrome/profile/Bookmarks
//       Explicit bookmarks source path (history not imported).
//
//   import-chrome /path/to/Bookmarks /path/to/bookmarks.json
//       Explicit bookmarks source and destination paths (history not imported).
//
//   import-chrome --history /path/to/History /path/to/history.json
//       Import only history with explicit source and destination paths.

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

// ── spatial-browser on-disk history format (mirrors persistence/history.rs) ──

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct HistoryEntry {
    url: String,
    visited_at: i64, // Unix seconds, UTC
}

#[derive(Default, Serialize, Deserialize)]
struct HistoryFile {
    entries: Vec<HistoryEntry>,
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
            // Build the path component for this folder's children. The root
            // nodes fed in from main() already carry meaningful display names
            // ("Bookmarks bar", "Other bookmarks", "Mobile bookmarks"), so we
            // always include them rather than skipping the first level.
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

fn default_chrome_history_path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/google-chrome/Default/History")
}

fn default_history_output_path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/history.json")
}

// ── Main ─────────────────────────────────────────────────────────────────────

/// Import Chrome history from `history_path` into `output_path`.
///
/// Chrome stores visit timestamps as microseconds since 1601-01-01 (Windows
/// FILETIME epoch). The offset from that epoch to the Unix epoch (1970-01-01)
/// is 11_644_473_600 seconds = 11_644_473_600_000_000 microseconds.
fn import_history(history_path: &PathBuf, output_path: &PathBuf) -> Result<()> {
    use rusqlite::{Connection, OpenFlags};

    // Open the SQLite file in read-only mode so we never corrupt Chrome's live
    // database. Chrome may have the file locked; if so we get a clear error.
    let conn = Connection::open_with_flags(
        history_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening history db {}", history_path.display()))?;

    // Microsecond offset between Windows FILETIME epoch and Unix epoch.
    const EPOCH_OFFSET_US: i64 = 11_644_473_600_000_000;

    let mut stmt = conn
        .prepare(
            "SELECT u.url, v.visit_time \
             FROM visits v \
             JOIN urls u ON u.id = v.url \
             WHERE v.visit_time > 0 \
             ORDER BY v.visit_time DESC",
        )
        .context("preparing history query")?;

    let imported: Vec<HistoryEntry> = stmt
        .query_map([], |row| {
            let url: String = row.get(0)?;
            let visit_time_us: i64 = row.get(1)?;
            // Convert Windows FILETIME microseconds → Unix seconds.
            let visited_at = (visit_time_us - EPOCH_OFFSET_US) / 1_000_000;
            Ok(HistoryEntry { url, visited_at })
        })
        .context("querying history")?
        .filter_map(|r| r.ok())
        .collect();

    eprintln!(
        "import-chrome: found {} history visit(s) in {}",
        imported.len(),
        history_path.display()
    );

    // Load existing history (or start empty).
    let existing: HistoryFile = std::fs::read(output_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    // Dedup by (url, visited_at) pair.
    let existing_set: HashSet<(&str, i64)> = existing
        .entries
        .iter()
        .map(|e| (e.url.as_str(), e.visited_at))
        .collect();
    let imported_count = imported.len();
    let new_entries: Vec<HistoryEntry> = imported
        .into_iter()
        .filter(|e| !existing_set.contains(&(e.url.as_str(), e.visited_at)))
        .collect();

    eprintln!(
        "import-chrome: {} new history visit(s) (skipping {} already present)",
        new_entries.len(),
        imported_count - new_entries.len(),
    );

    let mut merged = existing.entries;
    merged.extend(new_entries);
    // Keep newest first, matching spatial-browser's history.json convention.
    merged.sort_unstable_by(|a, b| b.visited_at.cmp(&a.visited_at));

    let result = HistoryFile { entries: merged };

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&result).context("serializing history")?;
    std::fs::write(output_path, bytes)
        .with_context(|| format!("writing {}", output_path.display()))?;

    eprintln!("import-chrome: wrote {}", output_path.display());
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // `import-chrome --history [<src> [<dst>]]` — import only history.
    if args.get(1).map(|s| s.as_str()) == Some("--history") {
        let history_path = args
            .get(2)
            .map(PathBuf::from)
            .unwrap_or_else(default_chrome_history_path);
        let output_path = args
            .get(3)
            .map(PathBuf::from)
            .unwrap_or_else(default_history_output_path);
        return import_history(&history_path, &output_path);
    }

    let chrome_path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_chrome_path);
    let output_path = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(default_output_path);

    // Read Chrome bookmarks.
    let chrome_bytes =
        std::fs::read(&chrome_path).with_context(|| format!("reading {}", chrome_path.display()))?;
    let chrome: ChromeBookmarksFile = serde_json::from_slice(&chrome_bytes)
        .with_context(|| format!("parsing {}", chrome_path.display()))?;

    // Collect all bookmarks from all three roots. The root nodes themselves
    // are treated as folders, so their names ("Bookmarks bar", etc.) appear
    // at the start of the path for their children.
    let mut imported: Vec<Bookmark> = Vec::new();
    for root in [
        &chrome.roots.bookmark_bar,
        &chrome.roots.other,
        &chrome.roots.synced,
    ] {
        collect(root, "", &mut imported);
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

    // When invoked with no arguments, also import history from the default
    // Chrome profile location alongside bookmarks.
    if args.len() == 1 {
        let history_path = default_chrome_history_path();
        let history_output = default_history_output_path();
        if history_path.exists() {
            if let Err(e) = import_history(&history_path, &history_output) {
                eprintln!("import-chrome: history import failed: {e:#}");
            }
        } else {
            eprintln!(
                "import-chrome: skipping history (not found at {})",
                history_path.display()
            );
        }
    }

    Ok(())
}
