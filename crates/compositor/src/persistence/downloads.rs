// downloads.json — completed CEF downloads for the Ctrl+J list.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_DOWNLOADS: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub url: String,
    pub path: String,
}

#[derive(Default, Serialize, Deserialize)]
struct DownloadsFile {
    entries: Vec<DownloadRecord>,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/downloads.json")
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
