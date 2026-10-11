// Page gallery for visual review: every built-in page, with sample data, in
// every theme, written as static HTML plus an index of iframes.
//
//   cargo test -p compositor -j 2 dump_gallery -- --ignored
//   → target/page-gallery/index.html (or $SPATIAL_GALLERY_DIR)
//
// When you add or change a page, add it to `samples` with data that shows
// its states (empty, long text, errors) so reviews catch layout problems.

use super::*;
use crate::output::THEMES;
use crate::persistence::bookmarks::Bookmark;
use crate::persistence::downloads::DownloadRecord;
use crate::persistence::history::HistoryEntry;

fn bookmarks() -> Vec<Bookmark> {
    serde_json::from_value(serde_json::json!([
        { "url": "https://github.com/Loafer19/spatial-browser", "title": "spatial-browser" },
        { "url": "https://docs.rs/maud/latest/maud/", "title": "maud — docs.rs", "folder": "Rust" },
        { "url": "https://doc.rust-lang.org/std/", "folder": "Rust" },
        { "url": "https://example.test/a/very/long/path/that/keeps/going/and/going?with=query&and=more", "title": "A bookmark with a very long title that should truncate cleanly instead of wrapping", "folder": "Reading" },
    ]))
    .expect("sample bookmarks")
}

fn history() -> Vec<HistoryEntry> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    [
        (
            "https://www.rust-lang.org/",
            Some("Rust Programming Language"),
            60,
        ),
        (
            "https://github.com/Loafer19/spatial-browser/pulls",
            Some("Pull requests"),
            600,
        ),
        ("https://news.ycombinator.com/", Some("Hacker News"), 4_000),
        ("https://example.test/no-title", None, 90_000),
        ("https://example.test/no-title", None, 90_100),
    ]
    .into_iter()
    .map(|(url, title, ago)| HistoryEntry {
        url: url.into(),
        visited_at: now.saturating_sub(ago) as _,
        title: title.map(Into::into),
    })
    .collect()
}

fn open_pages() -> Vec<palette::OpenPage> {
    [
        (1, "https://github.com/Loafer19/spatial-browser", Some("Loafer19/spatial-browser: a canvas of live web pages"), true),
        (2, "https://docs.rs/maud/latest/maud/", Some("maud - Rust"), false),
        (3, "https://example.test/a/very/long/path/that/keeps/going/and/going?with=query&and=more#and-a-fragment", None, false),
    ]
    .into_iter()
    .map(|(id, url, title, current)| palette::OpenPage {
        id,
        url: url.into(),
        title: title.map(Into::into),
        current,
    })
    .collect()
}

/// `(name, served URL)` for every page in `theme`.
fn samples(theme: &Theme) -> Vec<(&'static str, String)> {
    let workspaces: crate::persistence::workspaces::WorkspaceStore = serde_json::from_str(
        r#"{"active":1,"slots":[{"id":1,"visited":true},{"id":2,"visited":false}]}"#,
    )
    .unwrap_or_else(|_| serde_json::from_str(r#"{"active":0,"slots":[]}"#).unwrap());
    vec![
        ("help", help::page_url(theme)),
        (
            "palette",
            palette::page_url(
                theme,
                &open_pages(),
                &bookmarks(),
                &history(),
                "https://duckduckgo.com/?q=",
            ),
        ),
        (
            "palette-empty",
            palette::page_url(theme, &[], &[], &[], "https://duckduckgo.com/?q="),
        ),
        (
            "omnibox",
            omnibox::page_url(
                theme,
                &["rust maud".into(), "github.com".into()],
                "https://duckduckgo.com/?q=",
            ),
        ),
        (
            "vault-unlock",
            passwords_list::unlock_url(theme, true, None),
        ),
        (
            "vault-unlock-error",
            passwords_list::unlock_url(theme, false, Some("Wrong master password")),
        ),
        (
            "vault-list",
            passwords_list::page_url(theme, &[], &[], None, None, "saved"),
        ),
        (
            "vault-generator",
            passwords_list::page_url(theme, &[], &[], Some("Xk9#mP2$vL"), None, "generator"),
        ),
        ("find-bar", find_bar::page_url(theme)),
        (
            "save-prompt",
            save_prompt::page_url(theme, "github.com", "user@example.test", false),
        ),
        (
            "context-menu",
            context_menu::page_url(
                theme,
                &context_menu::MenuContext {
                    link: Some("https://example.test/link".into()),
                    ..Default::default()
                },
            ),
        ),
        (
            "bookmarks",
            bookmarks_list::page_url(theme, &bookmarks(), None),
        ),
        (
            "bookmarks-empty",
            bookmarks_list::page_url(theme, &[], None),
        ),
        (
            "downloads",
            downloads_list::page_url(
                theme,
                &[DownloadRecord {
                    url: "https://example.test/files/report-2026-10.pdf".into(),
                    path: "/home/user/Downloads/report-2026-10.pdf".into(),
                }],
            ),
        ),
        ("history", history_list::page_url(theme, &history())),
        ("workspaces", workspace_list::page_url(theme, &workspaces)),
        (
            "settings",
            settings_list::page_url(theme, &Default::default()),
        ),
        ("userscripts", userscripts_list::page_url(theme, &[], &[])),
    ]
}

fn slug(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[test]
#[ignore = "writes files; run on demand for visual review"]
fn dump_gallery() {
    let dir = std::env::var_os("SPATIAL_GALLERY_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/page-gallery")
        });
    std::fs::create_dir_all(&dir).expect("gallery dir");
    let mut index = String::from(
        "<!doctype html><meta charset=utf-8><title>Page gallery</title>\
         <style>body{margin:0;padding:20px;background:#111;color:#ddd;font:13px system-ui}\
         h2{margin:28px 0 10px}.g{display:grid;grid-template-columns:repeat(auto-fill,minmax(520px,1fr));gap:16px}\
         figure{margin:0}figcaption{margin:0 0 6px;opacity:.7}\
         iframe{width:100%;height:420px;border:1px solid #333;border-radius:6px;background:#fff}</style>",
    );
    for theme in THEMES {
        let theme_dir = slug(theme.name);
        std::fs::create_dir_all(dir.join(&theme_dir)).expect("theme dir");
        index.push_str(&format!("<h2>{}</h2><div class=g>", theme.name));
        for (name, url) in samples(theme) {
            let file = format!("{theme_dir}/{name}.html");
            std::fs::write(dir.join(&file), tests::served_html(&url)).expect("write page");
            index.push_str(&format!(
                "<figure><figcaption>{name}</figcaption><iframe src=\"{file}\" loading=lazy></iframe></figure>"
            ));
        }
        index.push_str("</div>");
    }
    std::fs::write(dir.join("index.html"), index).expect("write index");
    eprintln!("gallery: {}", dir.join("index.html").display());
}
