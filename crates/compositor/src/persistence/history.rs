// history.json — completed top-level visits (not omnibox typed_history).
//
// Kept for MAX_AGE_SECS or MAX_ENTRIES visits, whichever is shorter. The
// list page renders only a page of rows and asks Rust for the rest
// (`history://search`), so a full history opens as fast as a short one.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Visits older than this are dropped (load and every record).
pub const MAX_AGE_SECS: i64 = 90 * 24 * 3600;
/// Hard cap even within MAX_AGE_SECS (heavy browsing days).
pub const MAX_ENTRIES: usize = 10_000;
/// Current `history.json` layout. v1 (no `version` field) capped at 200
/// rows and was always newest-first; v2 is the same rows with a version
/// tag and the longer retention.
const FILE_VERSION: u32 = 2;

#[derive(Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    // Unix seconds UTC; local display/grouping is client-side JS.
    pub visited_at: i64,
    /// Page title when known (older files have none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Longest title kept (page titles can be arbitrary text).
const MAX_TITLE_CHARS: usize = 300;

fn clean_title(title: &str) -> Option<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty()).then(|| title.chars().take(MAX_TITLE_CHARS).collect())
}

#[derive(Default, Serialize, Deserialize)]
struct HistoryFile {
    /// Missing in v1 files.
    #[serde(default)]
    version: u32,
    entries: Vec<HistoryEntry>,
}

fn path() -> PathBuf {
    crate::persistence::config_path("history.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
/// Older files are migrated (and re-saved) to the current version.
pub fn load() -> Vec<HistoryEntry> {
    let Some(file) = super::store::load_json::<HistoryFile>(&path(), "history") else {
        return Vec::new();
    };
    let (history, changed) = migrate(file, super::vault::now_unix());
    if changed {
        save(&history);
    }
    history
}

/// Bring a loaded file up to FILE_VERSION and apply retention. True when
/// the result differs from what is on disk.
fn migrate(file: HistoryFile, now: i64) -> (Vec<HistoryEntry>, bool) {
    let mut history = file.entries;
    let mut changed = file.version != FILE_VERSION;
    // Newest first is an invariant every reader relies on (title updates,
    // day grouping); restore it if a hand-edited or old file broke it.
    if !history.is_sorted_by(|a, b| a.visited_at >= b.visited_at) {
        history.sort_by_key(|e| std::cmp::Reverse(e.visited_at));
        changed = true;
    }
    changed |= prune(&mut history, now);
    (history, changed)
}

/// Drop visits older than MAX_AGE_SECS and beyond MAX_ENTRIES (list is
/// newest first). True when anything was removed.
pub fn prune(history: &mut Vec<HistoryEntry>, now: i64) -> bool {
    let before = history.len();
    let cutoff = now.saturating_sub(MAX_AGE_SECS);
    let keep = history
        .iter()
        .position(|e| e.visited_at < cutoff)
        .unwrap_or(history.len())
        .min(MAX_ENTRIES);
    history.truncate(keep);
    history.len() != before
}

/// One search result: the row's index in the full list (for
/// `history://open|remove/<index>`), the entry, and how many matching
/// visits it stands for (1 unless collapsed).
pub struct Hit<'a> {
    pub index: usize,
    pub entry: &'a HistoryEntry,
    pub count: usize,
}

/// Rows matching `query` (every whitespace-separated word must appear in
/// the title or URL, case-insensitive), newest first: `limit` of them
/// starting at `offset`, plus the total row count. With `collapse`, each
/// URL is one row (its latest visit) carrying its visit count.
pub fn search<'a>(
    history: &'a [HistoryEntry],
    query: &str,
    offset: usize,
    limit: usize,
    collapse: bool,
) -> (Vec<Hit<'a>>, usize) {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let matching = history
        .iter()
        .enumerate()
        .filter(|(_, e)| matches(e, &words));
    let rows: Vec<Hit<'a>> = if collapse {
        // First occurrence = latest visit (newest first); later ones count.
        let mut slot: std::collections::HashMap<&str, usize> = Default::default();
        let mut rows: Vec<Hit<'a>> = Vec::new();
        for (index, entry) in matching {
            match slot.get(entry.url.as_str()) {
                Some(&i) => rows[i].count += 1,
                None => {
                    slot.insert(&entry.url, rows.len());
                    rows.push(Hit {
                        index,
                        entry,
                        count: 1,
                    });
                }
            }
        }
        rows
    } else {
        matching
            .map(|(index, entry)| Hit {
                index,
                entry,
                count: 1,
            })
            .collect()
    };
    let total = rows.len();
    (rows.into_iter().skip(offset).take(limit).collect(), total)
}

/// Remove every visit of `url`. True when anything was removed.
pub fn remove_url(history: &mut Vec<HistoryEntry>, url: &str) -> bool {
    let before = history.len();
    history.retain(|e| e.url != url);
    history.len() != before
}

pub(crate) fn matches(entry: &HistoryEntry, words: &[String]) -> bool {
    if words.is_empty() {
        return true;
    }
    let url = entry.url.to_lowercase();
    let title = entry.title.as_deref().unwrap_or("").to_lowercase();
    words
        .iter()
        .all(|w| url.contains(w.as_str()) || title.contains(w.as_str()))
}

/// Record visit at front; refresh timestamp (and title) if the same URL is
/// already on top.
pub fn record(history: &mut Vec<HistoryEntry>, url: &str, visited_at: i64, title: Option<&str>) {
    let title = title.and_then(clean_title);
    match history.first_mut() {
        Some(top) if top.url == url => {
            top.visited_at = visited_at;
            if title.is_some() {
                top.title = title;
            }
        }
        _ => history.insert(
            0,
            HistoryEntry {
                url: url.to_string(),
                visited_at,
                title,
            },
        ),
    }
    prune(history, visited_at);
    save(history);
}

/// A page's title changed (often after load-end, e.g. SPAs): set it on its
/// most recent visit and save. False when no recent visit of `url` exists.
pub fn set_title(history: &mut [HistoryEntry], url: &str, title: &str) -> bool {
    match apply_title(history, url, title) {
        TitleChange::NoVisit => false,
        TitleChange::Unchanged => true,
        TitleChange::Updated => {
            save(history);
            true
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TitleChange {
    NoVisit,
    Unchanged,
    Updated,
}

fn apply_title(history: &mut [HistoryEntry], url: &str, title: &str) -> TitleChange {
    let Some(title) = clean_title(title) else {
        return TitleChange::Unchanged;
    };
    // Only recent rows: an old visit of the same URL keeps its own title.
    let Some(entry) = history.iter_mut().take(20).find(|e| e.url == url) else {
        return TitleChange::NoVisit;
    };
    if entry.title.as_deref() == Some(title.as_str()) {
        return TitleChange::Unchanged;
    }
    entry.title = Some(title);
    TitleChange::Updated
}

pub fn save(history: &[HistoryEntry]) {
    let data = HistoryFile {
        version: FILE_VERSION,
        entries: history.to_vec(),
    };
    super::store::save_json_bg(&path(), "history", &data, super::store::Backup::None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, title: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            url: url.into(),
            visited_at: 0,
            title: title.map(Into::into),
        }
    }

    #[test]
    fn old_files_without_titles_still_load() {
        let file: HistoryFile =
            serde_json::from_str(r#"{"entries":[{"url":"https://a.test/","visited_at":1}]}"#)
                .unwrap();
        assert_eq!(file.entries[0].title, None);
    }

    fn at(url: &str, visited_at: i64) -> HistoryEntry {
        HistoryEntry {
            url: url.into(),
            visited_at,
            title: None,
        }
    }

    const NOW: i64 = 2_000_000_000;

    #[test]
    fn v1_file_migrates_to_v2() {
        let file: HistoryFile = serde_json::from_str(&format!(
            r#"{{"entries":[{{"url":"https://a.test/","visited_at":{}}}]}}"#,
            NOW - 10
        ))
        .unwrap();
        assert_eq!(file.version, 0);
        let (h, changed) = migrate(file, NOW);
        assert!(changed, "v1 is re-saved as v2");
        assert_eq!(h.len(), 1);
        let current = HistoryFile {
            version: FILE_VERSION,
            entries: h,
        };
        let json = serde_json::to_string(&current).unwrap();
        let reread: HistoryFile = serde_json::from_str(&json).unwrap();
        assert!(!migrate(reread, NOW).1, "v2 within limits is left alone");
    }

    #[test]
    fn migrate_restores_newest_first() {
        let file = HistoryFile {
            version: FILE_VERSION,
            entries: vec![
                at("https://old.test/", NOW - 50),
                at("https://new.test/", NOW - 5),
            ],
        };
        let (h, changed) = migrate(file, NOW);
        assert!(changed);
        assert_eq!(h[0].url, "https://new.test/");
    }

    #[test]
    fn prune_drops_old_and_excess() {
        let mut h = vec![
            at("https://a.test/", NOW),
            at("https://b.test/", NOW - MAX_AGE_SECS + 1),
            at("https://c.test/", NOW - MAX_AGE_SECS - 1),
        ];
        assert!(prune(&mut h, NOW));
        assert_eq!(h.len(), 2);
        let mut big: Vec<_> = (0..MAX_ENTRIES as i64 + 5)
            .map(|i| at("https://x.test/", NOW - i))
            .collect();
        assert!(prune(&mut big, NOW));
        assert_eq!(big.len(), MAX_ENTRIES);
        assert!(!prune(&mut big, NOW));
    }

    #[test]
    fn search_pages_through_all_matches() {
        let h: Vec<_> = (0..250)
            .map(|i| HistoryEntry {
                url: format!("https://site{}.test/page", i % 5),
                visited_at: NOW - i,
                title: Some(format!("Title {i}")),
            })
            .collect();
        let (hits, total) = search(&h, "SITE3", 0, 20, false);
        assert_eq!(total, 50);
        assert_eq!(hits.len(), 20);
        assert_eq!(hits[0].index, 3);
        let (hits, _) = search(&h, "site3", 40, 20, false);
        assert_eq!(hits.len(), 10);
        // Every word must match, title or URL.
        let (_, total) = search(&h, "title site1", 0, 10, false);
        assert_eq!(total, 50);
        let (_, total) = search(&h, "title nope", 0, 10, false);
        assert_eq!(total, 0);
        let (hits, total) = search(&h, "  ", 0, 10, false);
        assert_eq!((hits.len(), total), (10, 250));
    }

    #[test]
    fn collapse_keeps_latest_visit_with_count() {
        let h = vec![
            at("https://a.test/", NOW),
            at("https://b.test/", NOW - 1),
            at("https://a.test/", NOW - 2),
            at("https://a.test/", NOW - 3),
            at("https://c.test/", NOW - 4),
        ];
        let (hits, total) = search(&h, "", 0, 10, true);
        assert_eq!(total, 3);
        let rows: Vec<_> = hits
            .iter()
            .map(|h| (h.entry.url.as_str(), h.index, h.count))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("https://a.test/", 0, 3),
                ("https://b.test/", 1, 1),
                ("https://c.test/", 4, 1)
            ]
        );
        let (hits, total) = search(&h, "", 1, 1, true);
        assert_eq!((hits[0].entry.url.as_str(), total), ("https://b.test/", 3));
        let (_, total) = search(&h, "", 0, 10, false);
        assert_eq!(total, 5, "uncollapsed keeps every visit");
        let mut h = h;
        assert!(remove_url(&mut h, "https://a.test/"));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn search_10k_is_fast() {
        let h: Vec<_> = (0..MAX_ENTRIES as i64)
            .map(|i| HistoryEntry {
                url: format!("https://host{}.example.test/path/{i}?q=x", i % 400),
                visited_at: NOW - i,
                title: Some(format!("Some reasonably long page title number {i}")),
            })
            .collect();
        let start = std::time::Instant::now();
        let (_, total) = search(&h, "host7 number", 0, 100, false);
        assert!(total > 0);
        let (_, total) = search(&h, "", 0, 100, true);
        assert_eq!(total, MAX_ENTRIES);
        // Unoptimized test build; release is far faster.
        assert!(start.elapsed().as_millis() < 300, "{:?}", start.elapsed());
    }

    #[test]
    fn titles_are_cleaned_and_capped() {
        assert_eq!(
            clean_title("  Hello \n  world  ").as_deref(),
            Some("Hello world")
        );
        assert_eq!(clean_title("   "), None);
        assert_eq!(
            clean_title(&"x".repeat(1000)).unwrap().chars().count(),
            MAX_TITLE_CHARS
        );
    }

    #[test]
    fn title_goes_to_the_latest_visit_only() {
        let mut h = vec![
            entry("https://a.test/", None),
            entry("https://b.test/", None),
            entry("https://a.test/", Some("Old")),
        ];
        assert_eq!(
            apply_title(&mut h, "https://a.test/", "New"),
            TitleChange::Updated
        );
        assert_eq!(h[0].title.as_deref(), Some("New"));
        assert_eq!(h[2].title.as_deref(), Some("Old"));
        assert_eq!(
            apply_title(&mut h, "https://a.test/", " New "),
            TitleChange::Unchanged
        );
        assert_eq!(
            apply_title(&mut h, "https://c.test/", "C"),
            TitleChange::NoVisit
        );
    }
}
