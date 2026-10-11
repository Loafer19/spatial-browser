// bookmarks.json — deliberate edits, separate from the live canvas session.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub url: String,
    // `None` until renamed — CEF only exposes document title via an
    // async callback we don't wire up, so `display_label` falls back to
    // the URL's host until the user sets one explicitly.
    #[serde(default)]
    pub title: Option<String>,
    // `None` = ungrouped. Set via the same rename form as `title`.
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct BookmarksFile {
    bookmarks: Vec<Bookmark>,
}

fn path() -> PathBuf {
    crate::persistence::config_path("bookmarks.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<Bookmark> {
    super::store::load_json::<BookmarksFile>(&path(), "bookmarks")
        .map(|f| f.bookmarks)
        .unwrap_or_default()
}

pub fn save(bookmarks: &[Bookmark]) {
    let data = BookmarksFile {
        bookmarks: bookmarks.to_vec(),
    };
    super::store::save_json(&path(), "bookmarks", &data, super::store::Backup::Keep);
}

/// Bare host from a URL — the fallback display label, and used to guess
/// a favicon location (`https://{host}/favicon.ico`).
pub fn host_of(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme)
}

/// What to show for a bookmark: its title if renamed, else its host.
pub fn display_label(bookmark: &Bookmark) -> &str {
    bookmark
        .title
        .as_deref()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| host_of(&bookmark.url))
}

/// Folder path as stored: `/`-separated, segments trimmed, empty ones
/// dropped. `None` = unfiled.
pub fn normalize_folder(folder: &str) -> Option<String> {
    let path: Vec<&str> = folder
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    (!path.is_empty()).then(|| path.join("/"))
}

/// `folder` is `root` or below it.
pub fn in_folder(folder: &str, root: &str) -> bool {
    folder == root
        || folder
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Remove the rows at `ids` (any order, duplicates and stale ids ignored).
/// Returns how many were removed.
pub fn delete_many(bookmarks: &mut Vec<Bookmark>, ids: &[usize]) -> usize {
    let mut ids: Vec<usize> = ids
        .iter()
        .copied()
        .filter(|&i| i < bookmarks.len())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    for &i in ids.iter().rev() {
        bookmarks.remove(i);
    }
    ids.len()
}

/// Put the rows at `ids` into `folder` (empty = unfiled).
pub fn move_to(bookmarks: &mut [Bookmark], ids: &[usize], folder: &str) -> usize {
    let folder = normalize_folder(folder);
    let mut moved = 0;
    for &i in ids {
        if let Some(b) = bookmarks.get_mut(i) {
            b.folder = folder.clone();
            moved += 1;
        }
    }
    moved
}

/// Rename folder `from` (and its subfolders) to `to`. An empty `to`
/// unfiles `from`'s own bookmarks and lifts its subfolders to the top.
pub fn rename_folder(bookmarks: &mut [Bookmark], from: &str, to: &str) -> usize {
    let Some(from) = normalize_folder(from) else {
        return 0;
    };
    let to = normalize_folder(to);
    let mut changed = 0;
    for b in bookmarks.iter_mut() {
        let Some(folder) = b.folder.as_deref() else {
            continue;
        };
        if !in_folder(folder, &from) {
            continue;
        }
        let tail = folder[from.len()..].trim_start_matches('/');
        b.folder = match (&to, tail.is_empty()) {
            (Some(to), true) => Some(to.clone()),
            (Some(to), false) => Some(format!("{to}/{tail}")),
            (None, true) => None,
            (None, false) => Some(tail.to_string()),
        };
        changed += 1;
    }
    changed
}

/// Every folder path and its ancestors, sorted, with direct bookmark counts.
pub fn folder_tree(bookmarks: &[Bookmark]) -> Vec<(String, usize)> {
    let mut map: std::collections::BTreeMap<String, usize> = Default::default();
    for b in bookmarks {
        let Some(folder) = b.folder.as_deref().and_then(normalize_folder) else {
            continue;
        };
        let mut prefix = String::new();
        for seg in folder.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(seg);
            map.entry(prefix.clone()).or_insert(0);
        }
        *map.entry(folder).or_insert(0) += 1;
    }
    map.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(url: &str, folder: Option<&str>) -> Bookmark {
        Bookmark {
            url: url.into(),
            title: None,
            folder: folder.map(Into::into),
        }
    }

    #[test]
    fn folders_normalize_and_nest() {
        assert_eq!(
            normalize_folder(" Rust / /Async "),
            Some("Rust/Async".into())
        );
        assert_eq!(normalize_folder(" / "), None);
        assert!(in_folder("Rust/Async", "Rust"));
        assert!(!in_folder("Rustacean", "Rust"));
        let marks = [
            bm("a", Some("Rust/Async")),
            bm("b", Some("Rust")),
            bm("c", None),
        ];
        assert_eq!(
            folder_tree(&marks),
            [("Rust".to_string(), 1), ("Rust/Async".to_string(), 1)]
        );
    }

    #[test]
    fn delete_many_ignores_stale_and_duplicate_ids() {
        let mut marks = vec![bm("0", None), bm("1", None), bm("2", None), bm("3", None)];
        assert_eq!(delete_many(&mut marks, &[3, 1, 1, 99]), 2);
        let urls: Vec<_> = marks.iter().map(|b| b.url.as_str()).collect();
        assert_eq!(urls, ["0", "2"]);
    }

    #[test]
    fn move_and_rename_folders() {
        let mut marks = vec![
            bm("a", Some("Rust")),
            bm("b", Some("Rust/Async")),
            bm("c", Some("Rustacean")),
        ];
        assert_eq!(move_to(&mut marks, &[2, 7], " News "), 1);
        assert_eq!(marks[2].folder.as_deref(), Some("News"));
        assert_eq!(rename_folder(&mut marks, "Rust", "Lang/Rust"), 2);
        assert_eq!(marks[1].folder.as_deref(), Some("Lang/Rust/Async"));
        assert_eq!(rename_folder(&mut marks, "Lang/Rust", ""), 2);
        assert_eq!(marks[0].folder, None);
        assert_eq!(marks[1].folder.as_deref(), Some("Async"));
        assert_eq!(move_to(&mut marks, &[0], ""), 1);
        assert_eq!(marks[0].folder, None);
    }
}
