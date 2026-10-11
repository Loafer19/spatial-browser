// Ctrl+T start page / omnibox (`omnibox://go` → PENDING_OMNIBOX): search
// box with suggestions, most visited sites, bookmarks and recent input.
// Everything shown comes from local stores; tiles go through the same
// `omnibox://go` as typed text.

use super::ui::{self, html, Icon, Markup};
use crate::output::Theme;
use crate::persistence::bookmarks::{self, Bookmark};
use crate::persistence::history::HistoryEntry;

const SCRIPT: &str = r#"
const PREFIXES = {
  '@g': 'https://www.google.com/search?q=',
  '@y': 'https://www.youtube.com/results?search_query=',
  '@ddg': 'https://duckduckgo.com/?q=',
  '@bing': 'https://www.bing.com/search?q=',
  '@wiki': 'https://en.wikipedia.org/w/index.php?search='
};
// From Settings (json_script, so no escaping by hand).
var DEFAULT_ENGINE = JSON.parse(document.getElementById('omni-engine').textContent);
function resolve(raw) {
  const trimmed = raw.trim();
  // Prefix lookup is case-insensitive (`@G rust` works same as `@g
  // rust`) — PREFIXES' own keys are already all-lowercase, so only the
  // matched prefix needs lowercasing before the lookup.
  const m = trimmed.match(/^(@[a-zA-Z]+)\s+(.*)$/);
  if (m && PREFIXES[m[1].toLowerCase()]) return PREFIXES[m[1].toLowerCase()] + encodeURIComponent(m[2]);
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(trimmed)) return trimmed;
  if (/^[^\s]+\.[^\s]+$/.test(trimmed)) return 'https://' + trimmed;
  return DEFAULT_ENGINE + encodeURIComponent(trimmed);
}
// Suggestions: typed text → `omnibox://suggest` → Rust → omniSuggest().
let suggestions = [], selected = -1, lastQuery = '', suggestTimer = null;
function askSuggest(value) {
  clearTimeout(suggestTimer);
  selected = -1;
  suggestTimer = setTimeout(() => {
    lastQuery = value;
    if (!value.trim()) { omniSuggest({ query: value, rows: [] }); return; }
    location = 'omnibox://suggest?q=' + encodeURIComponent(value);
  }, 80);
}
function omniSuggest(res) {
  if (res.query !== lastQuery) return; // stale
  suggestions = res.rows;
  selected = -1;
  renderSuggest();
}
function renderSuggest() {
  const box = document.getElementById('suggest');
  box.textContent = '';
  suggestions.forEach((s, i) => {
    const row = document.createElement('div');
    row.className = 'sg' + (i === selected ? ' sg-active' : '');
    const main = document.createElement('span');
    main.className = 'sg-text';
    main.textContent = s.text;
    row.appendChild(main);
    if (s.title) {
      const t = document.createElement('span');
      t.className = 'sg-title';
      t.textContent = s.title;
      row.appendChild(t);
    }
    if (s.typed) {
      const t = document.createElement('span');
      t.className = 'sg-title';
      t.textContent = 'typed';
      row.appendChild(t);
    }
    row.onmousedown = e => { e.preventDefault(); go(s.text); };
    box.appendChild(row);
  });
}
function omniKey(event) {
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    if (!suggestions.length) return;
    event.preventDefault();
    const n = suggestions.length;
    // -1 = the typed text itself.
    selected = event.key === 'ArrowDown'
      ? (selected + 1 >= n ? -1 : selected + 1)
      : (selected - 1 < -1 ? n - 1 : selected - 1);
    renderSuggest();
  } else if (event.key === 'Escape' && selected >= 0) {
    event.preventDefault();
    selected = -1;
    renderSuggest();
  }
}
// Not `submit`: inside the form's inline onsubmit, `submit` resolves to
// HTMLFormElement.submit() (native submit, no go()) before any global.
function omniSubmit() {
  const q = document.getElementById('q');
  go(selected >= 0 && suggestions[selected] ? suggestions[selected].text : q.value);
}
function go(raw) {
  if (!raw) return;
  const dest = resolve(raw);
  window.location = 'omnibox://go?q=' + encodeURIComponent(raw) + '&url=' + encodeURIComponent(dest);
}
document.addEventListener('click', e => {
  const t = e.target.closest && e.target.closest('[data-go]');
  if (t) { e.preventDefault(); go(t.dataset.go); }
});
// Enter on a selected tile row (ui.js arrow keys) opens it.
document.addEventListener('keydown', e => {
  if (e.key !== 'Enter' || e.target.id === 'q') return;
  const row = window.ui && ui.active();
  if (row && row.dataset.go) { e.preventDefault(); go(row.dataset.go); }
});
"#;

const CSS: &str = r#"
.start { max-width: 760px; margin: 0 auto; padding-top: 9vh; }
.omni { display: flex; align-items: center; gap: 10px; padding: 0 16px; height: 54px;
  border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); }
.omni:focus-within { border-color: var(--accent); box-shadow: var(--ring); }
.omni svg { width: 22px; height: 22px; opacity: .6; flex-shrink: 0; }
.omni input { flex: 1; min-width: 0; border: 0 !important; outline: 0; box-shadow: none !important;
  background: transparent !important; color: var(--fg); font: inherit; font-size: 18px; padding: 0; height: auto; }
#suggest { margin-top: 6px; }
.sg { display: flex; gap: 12px; align-items: baseline; padding: 8px 16px; border-radius: var(--radius-sm);
  cursor: pointer; overflow: hidden; white-space: nowrap; }
.sg:hover, .sg-active { background: var(--accent); color: var(--accent-fg); }
.sg-text { overflow: hidden; text-overflow: ellipsis; font-family: var(--mono); font-size: 14px; }
.sg-title { opacity: .65; font-size: 13px; overflow: hidden; text-overflow: ellipsis; }
.hint { margin: 10px 4px 0; }
.tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(104px, 1fr)); gap: 10px; }
.tile { display: flex; flex-direction: column; align-items: center; gap: 8px; padding: 14px 8px;
  border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface);
  cursor: pointer; min-width: 0; text-align: center; }
.tile:hover, .tile.is-active { border-color: var(--accent); }
.tile .name { font-size: 12.5px; width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.chips { display: flex; flex-wrap: wrap; gap: 6px; }
.chip { border: 1px solid var(--border); background: var(--surface); color: var(--fg); border-radius: 999px;
  padding: 5px 12px; font: inherit; font-size: 13px; cursor: pointer; max-width: 260px;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.chip:hover { border-color: var(--accent); }
.keys-line { margin-top: 28px; text-align: center; }
"#;

/// Tiles shown for most visited sites / bookmarks.
pub(crate) const MAX_TILES: usize = 8;

/// One start-page tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopSite {
    pub url: String,
    pub title: String,
    pub host: String,
    pub visits: usize,
}

/// Most visited hosts in `history` (newest first): each host once, linked
/// to its most visited URL, ties to the more recent.
pub(crate) fn top_sites(history: &[HistoryEntry], limit: usize) -> Vec<TopSite> {
    use std::collections::HashMap;
    /// visits, first index seen (recency rank), per-URL visits.
    type HostStats<'a> = (usize, usize, Vec<(&'a HistoryEntry, usize)>);
    let mut hosts: HashMap<&str, HostStats<'_>> = HashMap::new();
    for (i, e) in history.iter().enumerate() {
        if !(e.url.starts_with("https://") || e.url.starts_with("http://")) {
            continue;
        }
        let host = bookmarks::host_of(&e.url);
        if host.is_empty() {
            continue;
        }
        let slot = hosts.entry(host).or_insert((0, i, Vec::new()));
        slot.0 += 1;
        match slot.2.iter_mut().find(|(u, _)| u.url == e.url) {
            Some((_, n)) => *n += 1,
            None => slot.2.push((e, 1)),
        }
    }
    let mut ranked: Vec<_> = hosts.into_iter().collect();
    ranked.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.1 .1.cmp(&b.1 .1)));
    ranked
        .into_iter()
        .take(limit)
        .map(|(host, (visits, _, urls))| {
            // Most visited URL; `max_by_key` keeps the last max, so reverse
            // to prefer the newest among equals.
            let (best, _) = urls
                .iter()
                .rev()
                .max_by_key(|(_, n)| *n)
                .copied()
                .unwrap_or((&history[0], 0));
            TopSite {
                url: best.url.clone(),
                title: best
                    .title
                    .clone()
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| host.to_string()),
                host: host.to_string(),
                visits,
            }
        })
        .collect()
}

fn tile(url: &str, host: &str, title: &str, sub: &str) -> Markup {
    html! {
        div class="tile row" data-nav data-go=(url) title=(format!("{title}\n{url}")) {
            (ui::favicon_lg(host, title))
            span class="name" { (title) }
            @if !sub.is_empty() { span class="tiny muted" { (sub) } }
        }
    }
}

/// What the start page shows besides the search box.
pub(crate) struct StartData<'a> {
    pub typed_history: &'a [String],
    pub history: &'a [HistoryEntry],
    pub bookmarks: &'a [Bookmark],
}

/// Start page / omnibox URL.
pub fn page_url(theme: &Theme, data: &StartData<'_>, default_search_engine: &str) -> String {
    let top = top_sites(data.history, MAX_TILES);
    let body = html! {
        main class="page start" {
            form onsubmit="omniSubmit();return false" {
                label class="omni" {
                    (Icon::Search)
                    input id="q" autofocus autocomplete="off" spellcheck="false"
                        oninput="askSuggest(this.value)" onkeydown="omniKey(event)"
                        aria-label="Search or type a URL"
                        placeholder="Search or type a URL — try @y, @ddg, @wiki, @bing";
                }
            }
            div id="suggest" {}
            @if !top.is_empty() {
                h2 { "Most visited" }
                div class="tiles" {
                    @for site in &top {
                        (tile(&site.url, &site.host, &site.title,
                            &format!("{} visit{}", site.visits, if site.visits == 1 { "" } else { "s" })))
                    }
                }
            }
            @if !data.bookmarks.is_empty() {
                h2 { "Bookmarks" }
                div class="tiles" {
                    @for b in data.bookmarks.iter().take(MAX_TILES) {
                        (tile(&b.url, bookmarks::host_of(&b.url), bookmarks::display_label(b),
                            b.folder.as_deref().unwrap_or("")))
                    }
                }
            }
            @if !data.typed_history.is_empty() {
                h2 { "Recent" }
                div class="chips" {
                    @for entry in data.typed_history.iter().take(10) {
                        button type="button" class="chip" data-go=(entry) title=(entry) { (entry) }
                    }
                }
            }
            p class="keys-line small muted" {
                kbd { "Ctrl+K" } " commands · " kbd { "Ctrl+B" } " bookmarks · "
                kbd { "Ctrl+H" } " history"
            }
            (ui::json_script("omni-engine", &serde_json::Value::from(default_search_engine)))
        }
    };
    let html = ui::Doc::new(theme, "New page")
        .no_row()
        .css(CSS)
        .script(SCRIPT)
        .render(body);
    super::serve(super::kind::OMNIBOX, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    /// Inline handlers resolve names against the element (and its form)
    /// before globals, so `onsubmit="submit()"` called the native
    /// HTMLFormElement.submit() and Enter never reached `go()` (v0.9.0).
    #[test]
    fn submit_handler_calls_our_function_not_a_form_method() {
        let data = StartData {
            typed_history: &[],
            history: &[],
            bookmarks: &[],
        };
        let html = served_html(&page_url(&THEMES[0], &data, "https://e.test/?q="));
        let start = html.find("onsubmit=\"").expect("form has onsubmit") + 10;
        let handler = &html[start..start + html[start..].find('"').unwrap()];
        assert_eq!(handler, "omniSubmit();return false");
        assert!(html.contains("function omniSubmit()"));
        assert!(html.contains("omnibox://go?q="));
        // Nothing in the page may define a global that a form/element
        // property would shadow inside inline handlers.
        for shadowed in ["submit", "reset", "action", "method", "elements"] {
            assert!(
                !html.contains(&format!("function {shadowed}(")),
                "global `{shadowed}` is shadowed by the form in inline handlers"
            );
        }
    }

    fn visit(url: &str, title: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            url: url.into(),
            visited_at: 0,
            title: title.map(Into::into),
        }
    }

    #[test]
    fn top_sites_group_by_host_and_skip_internal_urls() {
        let history = [
            visit("https://a.test/x", Some("X")),
            visit("https://b.test/", None),
            visit("https://a.test/y", None),
            visit("https://a.test/x", Some("X")),
            visit("spatial://page/abc", None),
            visit("file:///etc/passwd", None),
            visit("https://b.test/", None),
            visit("https://c.test/", None),
        ];
        let top = top_sites(&history, 8);
        let hosts: Vec<_> = top.iter().map(|t| t.host.as_str()).collect();
        assert_eq!(hosts, ["a.test", "b.test", "c.test"]);
        assert_eq!(
            (top[0].url.as_str(), top[0].visits),
            ("https://a.test/x", 3)
        );
        assert_eq!(top[1].title, "b.test");
        assert_eq!(top_sites(&history, 1).len(), 1);
    }

    #[test]
    fn start_page_tiles_are_escaped_and_engine_is_json() {
        let history = [visit(
            "https://a.test/\"><script>x()</script>",
            Some("<img src=x>"),
        )];
        let marks: Vec<Bookmark> =
            serde_json::from_value(serde_json::json!([{ "url": "https://b.test/'\"<b>" }]))
                .unwrap();
        let data = StartData {
            typed_history: &["</script><i>".into()],
            history: &history,
            bookmarks: &marks,
        };
        let html = served_html(&page_url(&THEMES[0], &data, "https://e.test/?q=</script>"));
        for raw in [
            "<script>x()",
            "<img src=x",
            "<b>",
            "</script><i>",
            "q=</script>",
        ] {
            assert!(!html.contains(raw), "{raw}");
        }
        assert!(
            html.contains("Most visited") && html.contains("Bookmarks") && html.contains("Recent")
        );
        assert!(html.contains("id=\"omni-engine\""));
    }
}
