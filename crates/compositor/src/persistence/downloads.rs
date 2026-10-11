// downloads.json — completed CEF downloads for the Ctrl+J list.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_DOWNLOADS: usize = 200;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub url: String,
    pub path: String,
    /// Server MIME type; absent in files written before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct DownloadsFile {
    entries: Vec<DownloadRecord>,
}

fn path() -> PathBuf {
    crate::persistence::config_path("downloads.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<DownloadRecord> {
    super::store::load_json::<DownloadsFile>(&path(), "downloads")
        .map(|f| f.entries)
        .unwrap_or_default()
}

/// Record completed download at front and save immediately.
pub fn record(downloads: &mut Vec<DownloadRecord>, entry: DownloadRecord) {
    downloads.insert(0, entry);
    downloads.truncate(MAX_DOWNLOADS);
    save(downloads);
}

pub fn save(downloads: &[DownloadRecord]) {
    let data = DownloadsFile {
        entries: downloads.to_vec(),
    };
    super::store::save_json_bg(&path(), "downloads", &data, super::store::Backup::None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_file_without_mime_loads() {
        let old =
            r#"{"entries":[{"url":"https://a.test/r.pdf","path":"/home/u/Downloads/r.pdf"}]}"#;
        let file: DownloadsFile = serde_json::from_str(old).expect("old downloads.json");
        assert_eq!(
            file.entries,
            [DownloadRecord {
                url: "https://a.test/r.pdf".into(),
                path: "/home/u/Downloads/r.pdf".into(),
                mime: None,
            }]
        );
    }

    #[test]
    fn mime_round_trips_and_is_omitted_when_unknown() {
        let entries = vec![
            DownloadRecord {
                url: "https://a.test/a.zip".into(),
                path: "/d/a.zip".into(),
                mime: Some("application/zip".into()),
            },
            DownloadRecord {
                url: "https://a.test/b".into(),
                path: "/d/b".into(),
                mime: None,
            },
        ];
        let json = serde_json::to_string(&DownloadsFile {
            entries: entries.clone(),
        })
        .unwrap();
        assert_eq!(json.matches("\"mime\"").count(), 1, "{json}");
        let back: DownloadsFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entries, entries);
    }
}
