// Ctrl+K command palette: open pages (`palette://go/<id>`), commands
// (`palette://run/<id>`), bookmarks and history (`palette://open?url=`), and
// typed URLs / searches (`omnibox://go`, the omnibox's own path). Rows are
// rendered here (maud escapes them); palette.js ranks, reorders, hides and
// highlights them client-side, so typing needs no round trip.

use std::collections::{HashMap, HashSet};

use super::ui::{self, html, Icon, Markup};
use crate::commands::{PaletteCommand, PALETTE};
use crate::output::Theme;
use crate::persistence::bookmarks::{self, host_of, Bookmark};
use crate::persistence::history::HistoryEntry;

/// Recent unique history URLs shipped to the page.
pub const HISTORY_CAP: usize = 300;
/// Bookmarks shipped to the page (an imported Chrome profile can hold
/// thousands; the first ones are the oldest and usually the most used).
pub const BOOKMARK_CAP: usize = 1000;
/// Total cached-favicon bytes inlined; rows past it keep the letter.
const ICON_BUDGET_BYTES: usize = 1536 * 1024;

const CSS: &str = include_str!("palette.css");
const SCRIPT: &str = concat!(include_str!("palette_rank.js"), include_str!("palette.js"));

/// An open, non-ephemeral tile.
pub struct OpenPage {
    /// CEF browser id (`palette://go/<id>`).
    pub id: i32,
    pub url: String,
    pub title: Option<String>,
    /// The tile that was focused when the palette opened.
    pub current: bool,
}

/// Palette page URL. `pages` topmost first; `history` newest first (as
/// stored); `default_search_engine` is the Settings search URL prefix.
pub fn page_url(
    theme: &Theme,
    pages: &[OpenPage],
    bookmarks: &[Bookmark],
    history: &[HistoryEntry],
    default_search_engine: &str,
) -> String {
    let history = recent_history(history, pages);
    let bookmarks = &bookmarks[..bookmarks.len().min(BOOKMARK_CAP)];
    let icons = icons(
        pages
            .iter()
            .map(|p| p.url.as_str())
            .chain(history.iter().map(|h| h.url.as_str()))
            .chain(bookmarks.iter().map(|b| b.url.as_str())),
    );
    let data = serde_json::json!({
        "engine": default_search_engine,
        "icons": icons,
    });

    let body = html! {
        div class="pal" {
            label class="pal-input" {
                (Icon::Search)
                span id="pal-scope" class="badge accent hidden" {}
                input id="q" type="text" autofocus autocomplete="off" spellcheck="false"
                    placeholder="Search pages, commands, bookmarks, history…"
                    aria-label="Command palette" aria-controls="results";
                kbd { "Esc" }
            }
            div id="results" class="pal-results" role="listbox" {
                section class="hidden" data-section="web" {
                    div class="pal-rows" {
                        div class="row hidden" data-nav data-web="go" {
                            span class="pal-icon" { (Icon::Globe) }
                            span class="main" { span class="title" {} span class="sub" {} }
                            kbd class="pal-enter" { "↵" }
                        }
                        div class="row hidden" data-nav data-web="search" {
                            span class="pal-icon" { (Icon::Search) }
                            span class="main" { span class="title" {} span class="sub" {} }
                            kbd class="pal-enter" { "↵" }
                        }
                    }
                }
                section data-section="pages" {
                    h2 class="section" { "Open pages" }
                    div class="pal-rows" {
                        @for page in pages { (page_row(page, &history)) }
                    }
                }
                section data-section="commands" {
                    h2 class="section" { "Commands" }
                    div class="pal-rows" {
                        @for command in PALETTE { (command_row(command)) }
                    }
                }
                section data-section="bookmarks" {
                    h2 class="section" { "Bookmarks" }
                    div class="pal-rows" {
                        @for bookmark in bookmarks { (bookmark_row(bookmark)) }
                    }
                }
                section data-section="history" {
                    h2 class="section" { "History" }
                    div class="pal-rows" {
                        @for (rank, entry) in history.iter().enumerate() { (history_row(entry, rank)) }
                    }
                }
                div id="pal-empty" class="pal-empty hidden" { "No matches" }
            }
            footer class="pal-foot" {
                span { kbd { "↑" } kbd { "↓" } "navigate" }
                span { kbd { "↵" } "open" }
                span class="opt" { kbd { ">" } "commands" }
                span class="opt" { kbd { "@" } "pages" }
                span class="spacer" {}
                span class="opt" { kbd { "Esc" } "close" }
            }
        }
        (ui::json_script("pal-data", &data))
    };
    let html = ui::Doc::new(theme, "Command palette")
        .class("compact palette")
        .escape("palette://close")
        .css(CSS)
        .script(SCRIPT)
        .render(body);
    super::serve(super::kind::PALETTE, html)
}

/// Favicon slot: the letter now, the cached icon (`icons[data-host]`) once
/// palette.js shows the row — a few hundred `<img>`s up front would be
/// wasted on rows nobody scrolls to.
fn favicon(url: &str, label: &str) -> Markup {
    let letter: String = label
        .chars()
        .find(|c| c.is_alphanumeric())
        .unwrap_or('?')
        .to_uppercase()
        .collect();
    let host = crate::persistence::favicons::key(url).unwrap_or_default();
    html! {
        span class="favicon" data-host=(host) aria-hidden="true" { (letter) }
    }
}

fn page_row(page: &OpenPage, history: &[&HistoryEntry]) -> Markup {
    let title = page
        .title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            history
                .iter()
                .find(|h| h.url == page.url)
                .and_then(|h| h.title.as_deref())
        })
        .unwrap_or_else(|| host_of(&page.url));
    let title = if title.is_empty() { "Untitled" } else { title };
    html! {
        div class="row" data-open={ "palette://go/" (page.id) } data-t=(title) data-u=(page.url) {
            (favicon(&page.url, title))
            span class="main" {
                span class="title" { (title) }
                span class="sub" { (page.url) }
            }
            @if page.current { span class="badge" { "Current" } }
        }
    }
}

fn command_row(command: &PaletteCommand) -> Markup {
    html! {
        div class="row" data-open={ "palette://run/" (command.id) }
            data-t=(command.title) data-k=(command.keywords)
            data-featured[command.featured] {
            span class="pal-icon" { (command.icon) }
            span class="main" { span class="title" { (command.title) } }
            @if let Some(shortcut) = command.shortcut {
                span class="pal-keys" aria-label=(shortcut) {
                    @for key in shortcut_keys(shortcut) { kbd { (key) } }
                }
            }
        }
    }
}

/// `Ctrl+Shift+T` → `Ctrl`, `Shift`, `T`; `Ctrl+,` keeps its `,` and a
/// trailing `+` (`Ctrl++`) is the key itself.
fn shortcut_keys(shortcut: &str) -> Vec<&str> {
    let mut keys = Vec::new();
    let mut rest = shortcut;
    while let Some(i) = rest.find('+') {
        if i == 0 {
            break;
        }
        keys.push(&rest[..i]);
        rest = &rest[i + 1..];
    }
    if !rest.is_empty() {
        keys.push(rest);
    }
    keys
}

fn bookmark_row(bookmark: &Bookmark) -> Markup {
    let title = bookmarks::display_label(bookmark);
    let folder = bookmark.folder.as_deref().filter(|f| !f.trim().is_empty());
    html! {
        div class="row" data-open={ "palette://open?url=" (super::url_component(&bookmark.url)) }
            data-t=(title) data-u=(bookmark.url) data-k=[folder] {
            (favicon(&bookmark.url, title))
            span class="main" {
                span class="title" { (title) }
                span class="sub" { (bookmark.url) }
            }
            @if let Some(folder) = folder { span class="badge" { (folder) } }
        }
    }
}

fn history_row(entry: &HistoryEntry, rank: usize) -> Markup {
    let title = entry
        .title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| host_of(&entry.url));
    html! {
        div class="row" data-open={ "palette://open?url=" (super::url_component(&entry.url)) }
            data-t=(title) data-u=(entry.url) data-r=(rank) data-at=(entry.visited_at) {
            (favicon(&entry.url, title))
            span class="main" {
                span class="title" { (title) }
                span class="sub" { (entry.url) }
            }
            span class="meta" {}
        }
    }
}

/// Newest visit of each web URL, newest first, capped. Pages already open
/// are left out: the Open pages row is the better hit.
fn recent_history<'a>(history: &'a [HistoryEntry], pages: &[OpenPage]) -> Vec<&'a HistoryEntry> {
    let open: HashSet<&str> = pages.iter().map(|p| p.url.as_str()).collect();
    let mut seen = HashSet::new();
    history
        .iter()
        .filter(|e| is_web_url(&e.url) && !open.contains(e.url.as_str()))
        .filter(|e| seen.insert(e.url.as_str()))
        .take(HISTORY_CAP)
        .collect()
}

fn is_web_url(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("file://")
}

/// Cached favicon data URIs for these URLs' hosts, first come first
/// served within ICON_BUDGET_BYTES.
fn icons<'a>(urls: impl Iterator<Item = &'a str>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut budget = ICON_BUDGET_BYTES;
    let mut seen = HashSet::new();
    for url in urls {
        let Some(key) = crate::persistence::favicons::key(url) else {
            continue;
        };
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Some(uri) = crate::persistence::favicons::data_uri(&key) {
            if uri.len() > budget {
                continue;
            }
            budget -= uri.len();
            out.insert(key, uri);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    const EVIL: &str = "\"><img src=x onerror=alert(1)></script><script>alert(2)</script>";

    fn visit(url: &str, title: Option<&str>, at: i64) -> HistoryEntry {
        HistoryEntry {
            url: url.into(),
            visited_at: at,
            title: title.map(Into::into),
        }
    }

    #[test]
    fn hostile_titles_and_urls_are_escaped() {
        let evil_url = format!("https://evil.test/{EVIL}");
        let pages = [OpenPage {
            id: 7,
            url: evil_url.clone(),
            title: Some(EVIL.into()),
            current: true,
        }];
        let bookmarks = vec![Bookmark {
            url: format!("https://b.test/{EVIL}"),
            title: Some(EVIL.into()),
            folder: Some(EVIL.into()),
        }];
        let history = vec![visit(&format!("https://h.test/?q={EVIL}"), Some(EVIL), 1)];
        let html = served_html(&page_url(
            &THEMES[0],
            &pages,
            &bookmarks,
            &history,
            "https://e.test/?q=\"</script>",
        ));
        assert!(!html.contains("<img src=x"), "{html}");
        assert!(!html.contains("<script>alert(2)"), "{html}");
        // Open actions carry the URL percent-encoded, never raw.
        assert!(html.contains("palette://open?url=https%3A%2F%2Fb.test%2F%22%3E%3Cimg"));
        assert!(html.contains("palette://go/7"));
        // The engine sits in the JSON block, which can't close its element.
        assert!(html.contains("https://e.test/?q=\\\"\\u003c/script\\u003e"));
    }

    #[test]
    fn every_command_row_runs_its_own_id() {
        let html = served_html(&page_url(&THEMES[0], &[], &[], &[], "https://e.test/?q="));
        for command in PALETTE {
            assert!(
                html.contains(&format!("data-open=\"palette://run/{}\"", command.id)),
                "{}",
                command.id
            );
        }
        assert!(html.contains("data-escape=\"palette://close\""));
        assert!(html.contains("<kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>T</kbd>"));
        assert!(html.contains("<kbd>Ctrl</kbd><kbd>,</kbd>"));
    }

    #[test]
    fn history_is_unique_recent_web_only_and_capped() {
        let mut history: Vec<HistoryEntry> = (0..HISTORY_CAP + 50)
            .map(|i| visit(&format!("https://site.test/{i}"), None, 10_000 - i as i64))
            .collect();
        history.insert(0, visit("https://site.test/1", Some("again"), 20_000));
        history.insert(0, visit("spatial://page/help/0123", None, 20_001));
        history.insert(0, visit("javascript:alert(1)", None, 20_002));
        history.insert(0, visit("https://open.test/", None, 20_003));
        let pages = [OpenPage {
            id: 1,
            url: "https://open.test/".into(),
            title: None,
            current: false,
        }];
        let recent = recent_history(&history, &pages);
        assert_eq!(recent.len(), HISTORY_CAP);
        assert_eq!(recent[0].title.as_deref(), Some("again"));
        assert_eq!(
            recent
                .iter()
                .filter(|e| e.url == "https://site.test/1")
                .count(),
            1
        );
        assert!(recent
            .iter()
            .all(|e| e.url.starts_with("https://site.test/")));
    }

    #[test]
    fn open_page_title_falls_back_to_history_then_host() {
        let history = [visit("https://a.test/x", Some("From history"), 1)];
        let refs: Vec<&HistoryEntry> = history.iter().collect();
        let row = |title: Option<&str>, url: &str| {
            page_row(
                &OpenPage {
                    id: 3,
                    url: url.into(),
                    title: title.map(Into::into),
                    current: false,
                },
                &refs,
            )
            .into_string()
        };
        assert!(row(Some("Live"), "https://a.test/x").contains("data-t=\"Live\""));
        assert!(row(Some("  "), "https://a.test/x").contains("data-t=\"From history\""));
        assert!(row(None, "https://b.test/y").contains("data-t=\"b.test\""));
        assert!(row(None, "").contains("data-t=\"Untitled\""));
    }

    #[test]
    fn shortcut_keys_split_on_plus() {
        assert_eq!(shortcut_keys("Ctrl+Shift+T"), ["Ctrl", "Shift", "T"]);
        assert_eq!(shortcut_keys("Ctrl+,"), ["Ctrl", ","]);
        assert_eq!(shortcut_keys("Ctrl++"), ["Ctrl", "+"]);
        assert_eq!(shortcut_keys("F1"), ["F1"]);
    }

    /// palette_rank.js under node (skipped when node isn't installed).
    #[test]
    fn ranking_prefers_prefix_and_word_starts_and_recent_history() {
        let test = r#"
const assert = (c, m) => { if (!c) { console.error('FAIL: ' + m); process.exit(1); } };
const R = PaletteRank;
const item = (t, u, k, r) => ({ t, u: u || '', k: k || '', r: r === undefined ? -1 : r });
const s = (it, q) => { const m = R.score(it, R.words(q)); return m ? m.score : -1; };
// Prefix > word start > inside > subsequence > no match.
assert(s(item('Settings'), 'set') > s(item('Reset canvas view'), 'set'), 'prefix beats inside');
assert(s(item('Reset canvas view'), 'can') > s(item('Scanner'), 'can'), 'word start beats inside');
assert(s(item('Scanner'), 'can') > s(item('Copy and Navigate'), 'can'), 'substring beats subsequence');
assert(s(item('Toggle reader mode'), 'trm') > 0, 'initials match');
assert(s(item('Settings'), 'xyz') < 0, 'no match');
assert(s(item('Bookmarks'), 'bkm') > 0, 'subsequence');
assert(s(item('abcdefghijklmnopqrstuvwx'), 'ax') < 0, 'too scattered');
assert(s(item('Open in system browser'), 'osb') > 0, 'initials across words');
// Every word must match somewhere; URL and keywords count.
assert(s(item('GitHub', 'https://github.com/x'), 'git com') > 0, 'words across fields');
assert(s(item('GitHub', 'https://github.com/x'), 'git zzz') < 0, 'all words required');
assert(s(item('Passwords', '', 'vault logins'), 'vault') > 0, 'keywords');
// Title hits rank above the same hit in the URL.
assert(s(item('Rust docs', 'https://x.test/'), 'rust') > s(item('Docs', 'https://rust.test/'), 'rust'), 'title over url');
// Recent history above old for the same match.
assert(s(item('News', 'https://n.test/', '', 0), 'news') > s(item('News', 'https://n.test/', '', 250), 'news'), 'recency');
// Highlights point at the matched characters.
const m = R.score(item('Reset canvas view'), R.words('can'));
assert(JSON.stringify(m.hits[0]) === '[6,7,8]', 'hits ' + JSON.stringify(m.hits));
// Prefixes.
assert(R.parse('>  set').scope === 'commands' && R.parse('>  set').text === 'set', 'commands prefix');
assert(R.parse('@gh').scope === 'pages' && R.parse('@gh').text === 'gh', 'pages prefix');
assert(R.parse('rust').scope === null, 'no prefix');
// URL detection and resolution, as the omnibox does it.
assert(R.looksLikeUrl('github.com') && R.looksLikeUrl('https://a.test/x y') === false, 'url-ish');
assert(R.looksLikeUrl('localhost:3000/a') && !R.looksLikeUrl('rust maud'), 'localhost / text');
assert(R.resolve('github.com', 'E?q=') === 'https://github.com', 'add https');
assert(R.resolve('localhost:8080', 'E?q=') === 'http://localhost:8080', 'localhost http');
assert(R.resolve('ftp://x.test/', 'E?q=') === 'ftp://x.test/', 'keep scheme');
assert(R.resolve('rust & maud', 'E?q=') === 'E?q=rust%20%26%20maud', 'search');
console.log('ok');
"#;
        let script = format!("{}\n{test}", include_str!("palette_rank.js"));
        let output = match std::process::Command::new("node")
            .arg("-e")
            .arg(&script)
            .output()
        {
            Ok(output) => output,
            Err(e) => {
                eprintln!("node not available ({e}); skipping palette ranking test");
                return;
            }
        };
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
