// Ctrl+H history list. Group/time formatting is client-side (local TZ via JS Date;
// avoids Rust timezone crates). `history://…` → PENDING_HISTORY_ACTION.

use super::ui::{self, html, Icon};
use crate::output::Theme;
use crate::persistence::bookmarks::host_of;
use crate::persistence::history::HistoryEntry;

/// Rows are built client-side from Rust-provided pages (`histResults`),
/// with local-time timestamps and client-side regrouping.
const SCRIPT: &str = r#"
function pad(n) { return String(n).padStart(2, '0'); }

let loaded = 0, total = 0, query = '', waiting = false, searchTimer = null;
let groupMode = 'flat';

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

function shortUrl(r) {
  const f = document.createDocumentFragment();
  if (r.http) {
    const b = el('span', 'badge danger http', 'http');
    b.title = 'Not encrypted (http)';
    f.appendChild(b);
  }
  f.appendChild(el('span', 'h-host', r.host));
  f.appendChild(el('span', 'h-rest', r.rest));
  return f;
}

function makeRow(r, icons) {
  const row = el('div', 'row');
  row.dataset.timestamp = r.t;
  row.dataset.host = r.host;
  row.dataset.open = 'history://open/' + r.i;
  // A collapsed row stands for every visit of its URL.
  row.dataset.delete = (r.n > 1 ? 'history://remove-all/' : 'history://remove/') + r.i;
  if (r.n > 1) row.dataset.confirm = 'Remove all ' + r.n + ' visits';
  row.title = r.url;
  const fav = el('span', 'favicon', (r.host.replace(/[^\p{L}\p{N}]/u, '')[0] || '?').toUpperCase());
  fav.setAttribute('aria-hidden', 'true');
  const icon = icons[r.host];
  if (typeof icon === 'string' && icon.startsWith('data:image/png;base64,')) {
    const img = el('img'); img.alt = ''; img.src = icon; fav.appendChild(img);
  }
  row.appendChild(fav);
  const main = el('div', 'main');
  if (r.title) {
    main.appendChild(el('span', 'title', r.title));
    const u = el('span', 'sub');
    u.appendChild(shortUrl(r));
    main.appendChild(u);
  } else {
    const t = el('span', 'title');
    t.appendChild(shortUrl(r));
    main.appendChild(t);
  }
  row.appendChild(main);
  if (r.n > 1) {
    const c = el('span', 'badge', '\u00d7' + r.n);
    c.title = r.n + ' visits';
    row.appendChild(c);
  }
  const d = new Date(r.t * 1000);
  row.dataset.date = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const meta = el('span', 'meta');
  meta.appendChild(el('span', 'h-date', row.dataset.date + ' '));
  meta.appendChild(el('span', '', `${pad(d.getHours())}:${pad(d.getMinutes())}`));
  row.appendChild(meta);
  const actions = el('div', 'actions');
  const remove = document.getElementById('trash-tpl').content.firstElementChild.cloneNode(true);
  remove.href = row.dataset.delete;
  remove.title = r.n > 1 ? 'Remove all ' + r.n + ' visits' : 'Remove from history';
  actions.appendChild(remove);
  row.appendChild(actions);
  return row;
}

// One page of rows from Rust (`history://search`); offset 0 replaces.
function histResults(res) {
  waiting = false;
  // Stale answer to an older search or the other view.
  if (res.query !== query || res.collapse !== (groupMode === 'flat')) return;
  const container = document.getElementById('rows');
  if (res.offset === 0) { container.textContent = ''; loaded = 0; }
  if (res.offset !== loaded) return;
  res.rows.forEach(r => container.appendChild(makeRow(r, res.icons)));
  loaded += res.rows.length;
  total = res.total;
  const more = document.getElementById('more');
  more.hidden = loaded >= total;
  more.textContent = `Show more (${total - loaded} left)`;
  document.getElementById('empty').hidden = !(total === 0 && query === '');
  document.getElementById('no-match').hidden = !(total === 0 && query !== '');
  document.querySelectorAll('[data-total]').forEach(e => { e.textContent = total; });
  applyGrouping(groupMode);
  if (res.offset === 0) ui.setActive(0, false);
}

function requestRows(offset) {
  waiting = true;
  location = 'history://search?q=' + encodeURIComponent(query) + '&offset=' + offset +
    (groupMode === 'flat' ? '&collapse=1' : '');
}

function showMore() { if (!waiting && loaded < total) requestRows(loaded); }

// "Recent" lists each URL once (latest visit, ×N); the grouped views list
// every visit, so switching between them reloads rows from Rust.
function setMode(mode) {
  const reload = (mode === 'flat') !== (groupMode === 'flat');
  groupMode = mode;
  if (reload) requestRows(0); else applyGrouping(mode);
}

function applyGrouping(mode) {
  groupMode = mode;
  const container = document.getElementById('rows');
  const rows = Array.from(container.querySelectorAll('.row'));
  container.querySelectorAll('.h-group').forEach(h => h.remove());
  container.classList.toggle('mode-date', mode === 'date');
  document.querySelectorAll('[data-mode]').forEach(b => {
    if (b.dataset.mode === mode) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current');
  });
  const addHeader = text => container.appendChild(el('h2', 'h-group', text));
  if (mode === 'flat') {
    rows.forEach(r => container.appendChild(r));
  } else if (mode === 'date') {
    // Rows are most-recent-first: a date change is a day boundary.
    let last = null;
    rows.forEach(r => {
      if (r.dataset.date !== last) { addHeader(r.dataset.date); last = r.dataset.date; }
      container.appendChild(r);
    });
  } else if (mode === 'host') {
    // Most recently visited host first; rows keep chronological order.
    const order = [];
    rows.forEach(r => { if (!order.includes(r.dataset.host)) order.push(r.dataset.host); });
    order.forEach(host => {
      addHeader(host);
      rows.filter(r => r.dataset.host === host).forEach(r => container.appendChild(r));
    });
  }
}

// Search runs in Rust over the whole history (every word must appear in
// the title or URL); debounced so typing doesn't flood navigations.
const box = document.querySelector('.search input');
box.addEventListener('input', () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { query = box.value.trim(); requestRows(0); }, 150);
});
box.addEventListener('keydown', e => {
  const list = ui.rows();
  const i = list.indexOf(ui.active());
  if (e.key === 'ArrowDown') { e.preventDefault(); ui.setActive(i + 1); }
  else if (e.key === 'ArrowUp') { e.preventDefault(); ui.setActive(i - 1); }
  else if (e.key === 'Enter') { e.preventDefault(); const r = ui.active(); if (r) ui.go(r.dataset.open); }
  else if (e.key === 'Escape' && box.value) {
    e.preventDefault(); e.stopPropagation(); box.value = ''; query = ''; requestRows(0);
  }
});
document.querySelectorAll('[data-mode]').forEach(b => b.addEventListener('click', e => {
  e.preventDefault(); setMode(b.dataset.mode);
}));
document.getElementById('more').addEventListener('click', showMore);
histResults(JSON.parse(document.getElementById('hist-initial').textContent));
// Load the next page when "Show more" scrolls into view.
new IntersectionObserver(entries => {
  if (entries.some(e => e.isIntersecting)) showMore();
}).observe(document.getElementById('more'));
"#;

const CSS: &str = r#"
#rows { display: flex; flex-direction: column; gap: 2px; }
#rows .row { cursor: pointer; }
.h-rest { opacity: .6; }
.h-group { margin-top: 16px; }
#rows.mode-date .h-date { display: none; }
.badge.http { margin-right: 6px; font-size: 10px; }
#more { display: block; margin: 16px auto 0; }
#more[hidden] { display: none; }
"#;

/// Rows per `history://search` answer (and on first paint).
pub(crate) const PAGE_ROWS: usize = 100;

/// How a URL is shown in the list: host without `www.` and the rest of
/// the URL dimmed after it (no scheme; a bare `/` dropped). `insecure` for
/// plain http, which gets a badge.
pub(crate) struct ShortUrl<'a> {
    pub host: &'a str,
    pub rest: &'a str,
    pub insecure: bool,
}

pub(crate) fn short_url(url: &str) -> ShortUrl<'_> {
    let (scheme, after) = url.split_once("://").unwrap_or(("", url));
    let split = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (host, rest) = after.split_at(split);
    let host = host.strip_prefix("www.").unwrap_or(host);
    let rest = if rest == "/" { "" } else { rest };
    ShortUrl {
        host,
        rest,
        insecure: scheme.eq_ignore_ascii_case("http"),
    }
}

/// JSON for `histResults`: one page of rows matching `query` from
/// `offset`, plus the favicons they need (once per host). Only ever handed
/// to the built-in history page, never to web content.
pub(crate) fn results_json(
    history: &[HistoryEntry],
    query: &str,
    offset: usize,
    collapse: bool,
) -> String {
    let (hits, total) =
        crate::persistence::history::search(history, query, offset, PAGE_ROWS, collapse);
    let mut icons = serde_json::Map::new();
    let rows: Vec<serde_json::Value> = hits
        .iter()
        .map(|hit| {
            let entry = hit.entry;
            let short = short_url(&entry.url);
            if !icons.contains_key(short.host) {
                if let Some(uri) = crate::persistence::favicons::data_uri(host_of(&entry.url)) {
                    icons.insert(short.host.to_string(), uri.into());
                }
            }
            serde_json::json!({
                "i": hit.index,
                "url": entry.url,
                "title": entry.title.as_deref().filter(|t| !t.is_empty()),
                "host": short.host,
                "rest": short.rest,
                "http": short.insecure,
                "t": entry.visited_at,
                "n": hit.count,
            })
        })
        .collect();
    serde_json::json!({
        "query": query,
        "collapse": collapse,
        "offset": offset,
        "total": total,
        "rows": rows,
        "icons": icons,
    })
    .to_string()
}

/// Builds the history-list page's spatial:// URL. `pub(crate)` so app.rs
/// can rebuild this page in place after a remove/clear. Only the first
/// PAGE_ROWS rows are embedded; the rest arrive via `history://search`.
pub(crate) fn page_url(theme: &Theme, history: &[HistoryEntry]) -> String {
    // Opens on "Recent", which shows each URL once.
    let initial: serde_json::Value =
        serde_json::from_str(&results_json(history, "", 0, true)).unwrap_or_default();
    let body = html! {
        main class="page" {
            (ui::head("History", Some(html! {
                span data-total { (history.len()) } " entries · kept 90 days · " kbd { "Del" } " remove"
            }), html! {
                (ui::search("q", "", "Search all history (title or URL)", None))
                a class="btn sm ghost danger" href="history://clear" { (Icon::Trash) "Clear all" }
            }))
            nav class="tabs" {
                a class="tab" href="#" data-mode="flat" aria-current="page" { "Recent" }
                a class="tab" href="#" data-mode="date" { "By day" }
                a class="tab" href="#" data-mode="host" { "By site" }
            }
            template id="trash-tpl" { (ui::icon_link("#", "Remove from history", Icon::Trash, true)) }
            div id="empty" hidden { (ui::empty(Icon::History, html! { "No history yet." })) }
            div id="rows" class="list" {}
            div id="no-match" hidden { (ui::empty(Icon::Search, html! { "No matches." })) }
            button id="more" class="btn sm" type="button" hidden { "Show more" }
            (ui::json_script("hist-initial", &initial))
        }
    };
    let html = ui::Doc::new(theme, "History")
        .no_row()
        .css(CSS)
        .script(SCRIPT)
        .render(body);
    super::serve(super::kind::HISTORY, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    #[test]
    fn short_url_drops_scheme_www_and_bare_slash() {
        let s = short_url("https://www.chess.com/home");
        assert_eq!((s.host, s.rest, s.insecure), ("chess.com", "/home", false));
        let s = short_url("https://edition.cnn.com/");
        assert_eq!((s.host, s.rest), ("edition.cnn.com", ""));
        let s = short_url("http://192.168.1.1/admin?x=1");
        assert_eq!(
            (s.host, s.rest, s.insecure),
            ("192.168.1.1", "/admin?x=1", true)
        );
        let s = short_url("https://a.test?q=1#f");
        assert_eq!((s.host, s.rest), ("a.test", "?q=1#f"));
    }

    #[test]
    fn rows_are_json_and_cannot_break_out_of_the_script() {
        let history = vec![
            HistoryEntry {
                url: "https://www.a.test/x\"><b>".into(),
                visited_at: 1,
                title: Some("</script><script>alert(1)</script>".into()),
            },
            HistoryEntry {
                url: "http://b.test/".into(),
                visited_at: 2,
                title: None,
            },
        ];
        let html = served_html(&page_url(&THEMES[0], &history));
        assert!(!html.contains("<script>alert"), "{html}");
        assert!(!html.contains("x\"><b>"));
        assert!(html.contains("\\u003c/script\\u003e"));
        let json: serde_json::Value =
            serde_json::from_str(&results_json(&history, "", 0, false)).unwrap();
        assert_eq!(json["total"], 2);
        assert_eq!(json["rows"][0]["host"], "a.test", "host without www.");
        assert_eq!(json["rows"][1]["http"], true);
        assert_eq!(json["rows"][1]["title"], serde_json::Value::Null);
    }

    #[test]
    fn recent_view_collapses_repeats() {
        let visit = |url: &str, t| HistoryEntry {
            url: url.into(),
            visited_at: t,
            title: None,
        };
        let history = vec![
            visit("https://a.test/", 3),
            visit("https://b.test/", 2),
            visit("https://a.test/", 1),
        ];
        let json: serde_json::Value =
            serde_json::from_str(&results_json(&history, "", 0, true)).unwrap();
        assert_eq!(json["total"], 2);
        assert_eq!(json["rows"][0]["n"], 2);
        assert_eq!(json["collapse"], true);
        let html = served_html(&page_url(&THEMES[0], &history));
        assert!(html.contains("\"collapse\":true"), "opens on Recent");
    }

    #[test]
    fn first_paint_embeds_one_page_and_counts_the_rest() {
        let history: Vec<_> = (0..10_000)
            .map(|i| HistoryEntry {
                url: format!("https://h{}.test/p/{i}", i % 300),
                visited_at: 2_000_000_000 - i,
                title: Some(format!("Page {i}")),
            })
            .collect();
        let start = std::time::Instant::now();
        let url = page_url(&THEMES[0], &history);
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 300, "{elapsed:?}");
        let html = served_html(&url);
        assert!(html.contains("\"total\":10000"));
        assert!(html.contains("\"i\":99,"));
        assert!(!html.contains("\"i\":100,"), "only PAGE_ROWS rows embedded");
        let json: serde_json::Value =
            serde_json::from_str(&results_json(&history, "h7.test", 10, false)).unwrap();
        assert_eq!(json["offset"], 10);
        assert_eq!(json["total"], 34);
        assert_eq!(json["rows"].as_array().unwrap().len(), 24);
    }
}
