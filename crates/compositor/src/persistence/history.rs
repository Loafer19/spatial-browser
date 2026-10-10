// history.json — completed top-level visits (not omnibox typed_history).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_HISTORY: usize = 200;

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
    entries: Vec<HistoryEntry>,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/history.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<HistoryEntry> {
    super::store::load_json::<HistoryFile>(&path(), "history")
        .map(|f| f.entries)
        .unwrap_or_default()
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
    history.truncate(MAX_HISTORY);
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
