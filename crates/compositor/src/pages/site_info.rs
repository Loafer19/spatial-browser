// Site info for the focused page (palette: "Site info"): connection, what
// this browser keeps about the site (history, bookmarks, saved logins),
// which scripts/styles and filters apply. Built from local stores for the
// built-in tile only; `site://` actions are accepted from built-in tiles.

use super::ui::{self, html, Icon, Markup};
use super::url_component;
use crate::output::Theme;
use crate::persistence::bookmarks::{self, Bookmark};
use crate::persistence::history::HistoryEntry;
use crate::persistence::settings::AppSettings;
use crate::persistence::vault::{self, VaultSession};
use crate::userscripts::{self, UserScript};
use crate::userstyles::{self, UserStyle};

/// Everything the page reads.
pub(crate) struct SiteContext<'a> {
    pub history: &'a [HistoryEntry],
    pub bookmarks: &'a [Bookmark],
    pub vault: Option<&'a VaultSession>,
    pub scripts: &'a [UserScript],
    pub styles: &'a [UserStyle],
    pub settings: &'a AppSettings,
    pub now: i64,
}

/// What the page shows, computed from `SiteContext` (tested separately).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SiteFacts {
    pub host: String,
    pub secure: bool,
    pub title: Option<String>,
    pub visits: usize,
    pub first_visit: Option<i64>,
    pub last_visit: Option<i64>,
    pub bookmarked: bool,
    pub host_bookmarks: usize,
    /// `None` while the vault is locked.
    pub logins: Option<usize>,
    pub never_save: Option<bool>,
    pub scripts: Vec<String>,
    pub styles: Vec<String>,
    pub blocked_host: bool,
}

pub(crate) fn facts(url: &str, cx: &SiteContext<'_>) -> SiteFacts {
    let host = bookmarks::host_of(url).to_string();
    let visits: Vec<&HistoryEntry> = cx
        .history
        .iter()
        .filter(|e| bookmarks::host_of(&e.url) == host)
        .collect();
    let title = cx
        .history
        .iter()
        .find(|e| e.url == url)
        .and_then(|e| e.title.clone())
        .filter(|t| !t.trim().is_empty());
    let origin = vault::normalize_origin(url);
    SiteFacts {
        secure: url.starts_with("https://"),
        title,
        visits: visits.len(),
        first_visit: visits.iter().map(|e| e.visited_at).min(),
        last_visit: visits.iter().map(|e| e.visited_at).max(),
        bookmarked: cx.bookmarks.iter().any(|b| b.url == url),
        host_bookmarks: cx
            .bookmarks
            .iter()
            .filter(|b| bookmarks::host_of(&b.url) == host)
            .count(),
        logins: cx.vault.map(|v| v.entries_for_origin(&origin).len()),
        never_save: cx.vault.map(|v| v.is_never_save(&origin)),
        scripts: cx
            .scripts
            .iter()
            .filter(|s| s.enabled && userscripts::url_matches_script(url, false, s))
            .map(|s| s.name.clone())
            .collect(),
        styles: cx
            .styles
            .iter()
            .filter(|s| s.enabled && userstyles::style_applies(url, false, s))
            .map(|s| s.name.clone())
            .collect(),
        blocked_host: cx
            .settings
            .custom_blocked_hosts
            .iter()
            .any(|h| host == *h || host.ends_with(&format!(".{h}"))),
        host,
    }
}

/// "today" / "3 days ago" relative to `now` (whole days, no time zone).
fn ago(now: i64, then: i64) -> String {
    match (now - then).max(0) / 86_400 {
        0 => "today".into(),
        1 => "yesterday".into(),
        d => format!("{d} days ago"),
    }
}

fn fact(icon: Icon, title: &str, value: Markup) -> Markup {
    html! {
        div class="row card" data-nav {
            span class="fact-icon" { (icon) }
            div class="main" { span class="sub" { (title) } span class="title" { (value) } }
        }
    }
}

const CSS: &str = r#"
.fact-icon { display: inline-flex; width: 22px; height: 22px; opacity: .7; }
.fact-icon svg { width: 22px; height: 22px; }
.url { font-family: var(--mono); font-size: 12px; word-break: break-all; }
"#;

pub(crate) fn page_url(theme: &Theme, url: &str, cx: &SiteContext<'_>) -> String {
    let f = facts(url, cx);
    let label = f.title.clone().unwrap_or_else(|| f.host.clone());
    let enc = url_component(url);
    let bookmark_href = format!(
        "site://toggle-bookmark?url={enc}&title={}",
        url_component(f.title.as_deref().unwrap_or(""))
    );
    let forget_href = format!("site://forget-history?url={enc}");
    let body = html! {
        main class="page" {
            header class="page-head" {
                (ui::favicon_lg(&f.host, &label))
                h1 { (f.host) }
                a class={ "btn sm" @if f.bookmarked { " primary" } } href=(bookmark_href) {
                    (Icon::Star) (if f.bookmarked { "Bookmarked" } else { "Bookmark" })
                }
                div class="sub small muted url" { (url) }
            }
            div class="list" {
                (fact(if f.secure { Icon::Lock } else { Icon::Info }, "Connection", html! {
                    @if f.secure { "Encrypted (HTTPS)" }
                    @else { span class="danger-text" { "Not encrypted (HTTP) — passwords are not offered here" } }
                }))
                (fact(Icon::History, "History", html! {
                    @if f.visits == 0 { "No visits kept" }
                    @else {
                        (f.visits) " visit" (if f.visits == 1 { "" } else { "s" })
                        @if let (Some(first), Some(last)) = (f.first_visit, f.last_visit) {
                            " · first " (ago(cx.now, first)) " · last " (ago(cx.now, last))
                        }
                    }
                }))
                (fact(Icon::Star, "Bookmarks on this site", html! { (f.host_bookmarks) }))
                (fact(Icon::Key, "Saved logins", html! {
                    @match (f.logins, f.never_save) {
                        (Some(n), Some(true)) => { (n) " · never offer to save here" }
                        (Some(n), _) => { (n) }
                        (None, _) => { "Vault locked" }
                    }
                }))
                (fact(Icon::Code, "Scripts & styles that run here", html! {
                    @if f.scripts.is_empty() && f.styles.is_empty() { "None" }
                    @else { (f.scripts.iter().chain(&f.styles).cloned().collect::<Vec<_>>().join(", ")) }
                }))
                (fact(Icon::Shield, "Content filtering", html! {
                    @if f.blocked_host { "This host is on your blocked list" }
                    @else if cx.settings.ad_block_enabled { "On" }
                    @else { "Off" }
                }))
            }
            h2 { "Actions" }
            div class="cluster" {
                @if f.visits > 0 {
                    a class="btn sm danger" href=(forget_href) { (Icon::Trash) "Forget history for " (f.host) }
                }
                a class="btn sm ghost" href="settings://tab/blocking" { (Icon::Shield) "Blocking settings" }
            }
        }
    };
    let html = ui::Doc::new(theme, "Site info")
        .no_row()
        .css(CSS)
        .render(body);
    super::serve(super::kind::SITE, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    fn visit(url: &str, t: i64) -> HistoryEntry {
        HistoryEntry {
            url: url.into(),
            visited_at: t,
            title: Some("A <b>".into()),
        }
    }

    #[test]
    fn facts_cover_history_bookmarks_and_locked_vault() {
        let history = [
            visit("https://a.test/x", 100),
            visit("https://a.test/y", 50),
            visit("https://b.test/", 70),
        ];
        let marks: Vec<Bookmark> =
            serde_json::from_value(serde_json::json!([{ "url": "https://a.test/x" }])).unwrap();
        let settings = AppSettings {
            custom_blocked_hosts: vec!["a.test".into()],
            ..Default::default()
        };
        let cx = SiteContext {
            history: &history,
            bookmarks: &marks,
            vault: None,
            scripts: &[],
            styles: &[],
            settings: &settings,
            now: 200,
        };
        let f = facts("https://a.test/x", &cx);
        assert_eq!(f.host, "a.test");
        assert!(f.secure && f.bookmarked && f.blocked_host);
        assert_eq!(
            (f.visits, f.first_visit, f.last_visit),
            (2, Some(50), Some(100))
        );
        assert_eq!((f.logins, f.never_save), (None, None));
        let html = served_html(&page_url(&THEMES[0], "https://a.test/x", &cx));
        assert!(!html.contains("A <b>"));
        assert!(html.contains("Vault locked"));
        assert!(html.contains("site://forget-history?url=https%3A%2F%2Fa.test%2Fx"));
        assert!(html
            .contains("site://toggle-bookmark?url=https%3A%2F%2Fa.test%2Fx&amp;title=A%20%3Cb%3E"));
    }

    #[test]
    fn unlocked_vault_counts_logins_only() {
        let mut e = vault::test_entry("1", "https://a.test");
        e.password = "secret-pw".into();
        let v = VaultSession::for_test(vec![e]);
        let settings = AppSettings::default();
        let cx = SiteContext {
            history: &[],
            bookmarks: &[],
            vault: Some(&v),
            scripts: &[],
            styles: &[],
            settings: &settings,
            now: 0,
        };
        assert_eq!(facts("https://a.test/login", &cx).logins, Some(1));
        let html = served_html(&page_url(&THEMES[0], "http://a.test/", &cx));
        assert!(!html.contains("secret-pw"));
        assert!(html.contains("Not encrypted"));
    }
}
