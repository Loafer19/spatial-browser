// Local favicon cache for the built-in pages (bookmarks, history,
// downloads, switcher): PNGs captured from the browser cache on visit
// (cef_bridge::fetch_favicon), served to the pages as data: URIs. Lists
// never fetch https://<host>/favicon.ico, which leaked each listed site.
//
// Keyed by vault::host_key (lowercase host[:port], no `www.`). The set of
// files is itself a list of visited sites: cleared with history.

use super::store::{self, Backup};
use super::vault;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Larger "icons" are ignored (a list tile draws 20 CSS px).
const MAX_ICON_BYTES: usize = 32 * 1024;
/// Re-capture an icon after this long.
const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);

thread_local! {
    // key → data: URI (None = known missing), so list pages don't hit disk.
    static MEMORY: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
}

fn dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/favicons")
}

/// Cache key for a page URL or bare host; None for non-web input.
pub fn key(url_or_host: &str) -> Option<String> {
    let key = vault::host_key(url_or_host);
    (!key.is_empty()).then_some(key)
}

/// File name for a key: host chars kept, anything else (the port colon,
/// IPv6 brackets) mapped to `_`, so a key can never escape the directory.
fn file_name(key: &str) -> String {
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.png", safe.trim_start_matches('.'))
}

/// Whether an icon for `key` was captured within REFRESH_AFTER.
pub fn is_fresh(key: &str) -> bool {
    std::fs::metadata(dir().join(file_name(key)))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < REFRESH_AFTER)
}

/// Store a captured PNG for `key` (oversized or non-PNG data is dropped).
pub fn store(key: &str, png: &[u8]) {
    if png.len() > MAX_ICON_BYTES || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        log::debug!(
            "favicon {key}: dropped ({} bytes, not a small PNG)",
            png.len()
        );
        return;
    }
    let path = dir().join(file_name(key));
    if let Err(e) = store::write_atomic(&path, png, Backup::None) {
        log::warn!("favicon {key}: couldn't write {path:?}: {e}");
        return;
    }
    log::info!("favicon cached for {key} ({} bytes)", png.len());
    MEMORY.with_borrow_mut(|m| m.insert(key.to_string(), Some(data_uri_of(png))));
}

/// `data:image/png;base64,…` for `host`, if one was captured.
pub fn data_uri(host: &str) -> Option<String> {
    let key = key(host)?;
    MEMORY.with_borrow_mut(|m| {
        m.entry(key.clone())
            .or_insert_with(|| {
                std::fs::read(dir().join(file_name(&key)))
                    .ok()
                    .filter(|b| b.len() <= MAX_ICON_BYTES && b.starts_with(b"\x89PNG\r\n\x1a\n"))
                    .map(|b| data_uri_of(&b))
            })
            .clone()
    })
}

/// Forget every icon (history cleared).
pub fn clear() {
    MEMORY.with_borrow_mut(|m| m.clear());
    if let Ok(entries) = std::fs::read_dir(dir()) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn data_uri_of(png: &[u8]) -> String {
    let mut out = String::from("data:image/png;base64,");
    base64_into(png, &mut out);
    out
}

/// Standard padded base64 (RFC 4648 §4).
fn base64_into(bytes: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    out.reserve(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_file_names_stay_inside_the_dir() {
        assert_eq!(
            key("https://www.Example.com/x").as_deref(),
            Some("example.com")
        );
        assert_eq!(key("data:text/html,x"), None);
        assert_eq!(file_name("localhost:3000"), "localhost_3000.png");
        assert_eq!(file_name("[::1]:8080"), "___1__8080.png");
        assert_eq!(file_name("../../etc"), "_.._etc.png");
        assert!(!file_name("a/b").contains('/'));
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            let mut out = String::new();
            base64_into(input.as_bytes(), &mut out);
            assert_eq!(out, expected);
        }
    }
}
