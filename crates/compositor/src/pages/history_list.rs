// Ctrl+H history list. Group/time formatting is client-side (local TZ via JS Date;
// avoids Rust timezone crates). `history://…` → PENDING_HISTORY_ACTION.

use super::{LIST_NAV_SCRIPT, TRASH_SVG_PATH};
use crate::output::Theme;
use crate::persistence::bookmarks::host_of;
use crate::persistence::history::HistoryEntry;

/// Plain const; rows are built client-side from Rust-provided pages
/// (`histResults`), local-time timestamps, client-side regrouping.
const SCRIPT: &str = r#"<script>
function pad(n) { return String(n).padStart(2, '0'); }

let loaded = 0, total = 0, query = '', waiting = false, searchTimer = null;

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

function shortUrl(r) {
  const f = document.createDocumentFragment();
  if (r.http) {
    const b = el('span', 'hist-http', 'http');
    b.title = 'Not encrypted (http)';
    f.appendChild(b);
  }
  f.appendChild(el('span', 'hist-host', r.host));
  f.appendChild(el('span', 'hist-rest', r.rest));
  return f;
}

function makeRow(r, icons) {
  const row = el('div', 'hist-row list-row');
  row.dataset.timestamp = r.t;
  row.dataset.host = r.host;
  row.dataset.open = 'history://open/' + r.i;
  row.title = r.url;
  row.onclick = () => { location = 'history://open/' + r.i; };
  const fav = el('span', 'fav');
  fav.appendChild(el('span', 'fav-letter', (r.host[0] || '?').toUpperCase()));
  const icon = icons[r.host];
  if (typeof icon === 'string' && icon.startsWith('data:image/png;base64,')) {
    const img = el('img');
    img.width = 20; img.height = 20; img.src = icon;
    fav.appendChild(img);
  }
  row.appendChild(fav);
  const text = el('span', 'hist-text');
  if (r.title) {
    text.appendChild(el('span', 'hist-title', r.title));
    const u = el('span', 'hist-url');
    u.appendChild(shortUrl(r));
    text.appendChild(u);
  } else {
    const t = el('span', 'hist-title');
    t.appendChild(shortUrl(r));
    text.appendChild(t);
  }
  row.appendChild(text);
  const d = new Date(r.t * 1000);
  row.dataset.date = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  row.appendChild(el('span', 'hist-date hist-when', row.dataset.date));
  row.appendChild(el('span', 'hist-time hist-when', `${pad(d.getHours())}:${pad(d.getMinutes())}`));
  const remove = document.getElementById('trash-tpl').content.firstElementChild.cloneNode(true);
  remove.href = 'history://remove/' + r.i;
  remove.onclick = e => e.stopPropagation();
  row.appendChild(remove);
  return row;
}

// One page of rows from Rust (`history://search`); offset 0 replaces.
function histResults(res) {
  waiting = false;
  if (res.query !== query) return; // stale answer to an older search
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
  applyGrouping(groupMode);
  if (res.offset === 0) setListActive(0);
}

function requestRows(offset) {
  waiting = true;
  location = 'history://search?q=' + encodeURIComponent(query) + '&offset=' + offset;
}

function showMore() {
  if (!waiting && loaded < total) requestRows(loaded);
}

let groupMode = 'flat';

function applyGrouping(mode) {
  groupMode = mode;
  const container = document.getElementById('rows');
  const rows = Array.from(container.querySelectorAll('.hist-row'));
  container.querySelectorAll('.hist-header').forEach(h => h.remove());
  container.classList.toggle('mode-date', mode === 'date');
  document.querySelectorAll('.group-toggle button').forEach(b => {
    b.classList.toggle('active', b.dataset.mode === mode);
  });

  const addHeader = text => {
    const h = document.createElement('div');
    h.className = 'hist-header';
    h.textContent = text;
    container.appendChild(h);
  };

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
function filterRows(value) {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { query = value.trim(); requestRows(0); }, 150);
}

function searchKey(event) {
  if (event.key === 'ArrowDown') { event.preventDefault(); setListActive(listActiveIndex + 1); }
  else if (event.key === 'ArrowUp') { event.preventDefault(); setListActive(listActiveIndex - 1); }
  else if (event.key === 'Enter') {
    const row = listRows()[listActiveIndex];
    if (row) location = row.dataset.open;
  } else if (event.key === 'Escape' && event.target.value) {
    event.preventDefault(); event.target.value = ''; filterRows('');
  }
}

function initHistory() {
  histResults(INITIAL);
  // Load the next page when the "Show more" button scrolls into view.
  new IntersectionObserver(entries => {
    if (entries.some(e => e.isIntersecting)) showMore();
  }).observe(document.getElementById('more'));
}
</script>"#;

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
pub(crate) fn results_json(history: &[HistoryEntry], query: &str, offset: usize) -> String {
    let (hits, total) = crate::persistence::history::search(history, query, offset, PAGE_ROWS);
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
            })
        })
        .collect();
    serde_json::json!({
        "query": query,
        "offset": offset,
        "total": total,
        "rows": rows,
        "icons": icons,
    })
    .to_string()
}

/// JSON made safe to sit inside a `<script>` element.
fn script_json(json: &str) -> String {
    json.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

/// Builds the history-list page's spatial:// URL. `pub(crate)` so app.rs
/// can rebuild this page in place after a remove/clear. Only the first
/// PAGE_ROWS rows are embedded; the rest arrive via `history://search`.
pub(crate) fn page_url(theme: &Theme, history: &[HistoryEntry]) -> String {
    let initial = script_json(&results_json(history, "", 0));
    let html = format!(
        "{script}{nav_script}<script>const INITIAL = {initial};</script>\
         <style>\
         .hist-row{{display:flex;align-items:center;gap:10px;padding:8px 14px;cursor:pointer;\
         background:{card_bg};border-radius:8px;border:1px solid {card_border}}}\
         .hist-row:hover,.hist-row.list-active{{background:{key_bg}!important}}\
         .hist-row:hover span,.hist-row.list-active span{{color:{key_fg}!important}}\
         {icon_hover}\
         .fav{{position:relative;width:20px;height:20px;flex-shrink:0}}\
         .fav-letter,.fav img{{position:absolute;inset:0;border-radius:4px}}\
         .fav-letter{{background:{key_bg};color:{key_fg};display:flex;align-items:center;\
         justify-content:center;font-size:11px;font-weight:700}}\
         .hist-when{{flex-shrink:0;color:{fg};opacity:0.6;font-size:12px;white-space:nowrap}}\
         .hist-header{{margin:16px 0 0;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;opacity:0.8;color:{heading}}}\
         .hist-header:first-child{{margin-top:0}}\
         #rows.mode-date .hist-date{{display:none}}\
         .hist-text{{flex:1;min-width:0;display:flex;flex-direction:column;gap:2px}}\
         .hist-title,.hist-url{{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:{fg}}}\
         .hist-url{{font-size:12px;opacity:0.65}}\
         .hist-rest{{opacity:0.55}}\
         .hist-http{{font-size:10px;padding:1px 5px;margin-right:6px;border-radius:4px;\
         border:1px solid {danger};color:{danger}!important;vertical-align:1px}}\
         #q{{width:100%;box-sizing:border-box;background:{card_bg};color:{fg};\
         border:1px solid {card_border};border-radius:8px;padding:8px 12px;font:inherit;\
         margin-bottom:12px}}\
         .group-toggle button,#more{{background:{card_bg};color:{fg};opacity:0.7;\
         border:1px solid {card_border};border-radius:6px;padding:5px 12px;\
         font:inherit;font-size:13px;cursor:pointer}}\
         #more{{display:block;margin:16px auto 0}}#more[hidden]{{display:none}}\
         .group-toggle button.active{{background:{key_bg};color:{key_fg};opacity:1;\
         border-color:{key_bg}}}\
         </style>\
         {body_open}\
         <template id=\"trash-tpl\">{trash}</template>\
         <div style=\"display:flex;align-items:baseline;justify-content:space-between;\
         margin:0 0 16px\">\
         <h1 style=\"margin:0;color:{heading};font-size:20px\">History</h1>\
         <a href=\"history://clear\" style=\"color:{fg};opacity:0.6;font-size:13px\">Clear all</a>\
         </div>\
         <input id=\"q\" type=\"search\" placeholder=\"Search all history (title or URL)\" autofocus \
         oninput=\"filterRows(this.value)\" onkeydown=\"searchKey(event)\">\
         <div class=\"group-toggle\" style=\"display:flex;gap:6px;margin-bottom:16px\">\
         <button data-mode=\"flat\" onclick=\"applyGrouping('flat')\" class=\"active\">Recent</button>\
         <button data-mode=\"date\" onclick=\"applyGrouping('date')\">By day</button>\
         <button data-mode=\"host\" onclick=\"applyGrouping('host')\">By site</button>\
         </div>\
         <div id=\"empty\" hidden>{empty}</div>\
         <div id=\"rows\" style=\"display:flex;flex-direction:column;gap:8px\"></div>\
         <p id=\"no-match\" hidden style=\"opacity:0.6\">No matches.</p>\
         <button id=\"more\" hidden onclick=\"showMore()\">Show more</button></body>",
        script = SCRIPT,
        nav_script = LIST_NAV_SCRIPT,
        icon_hover = super::icon_button_hover_css(theme),
        body_open = super::body_open(theme, "onload=\"initHistory()\""),
        trash = super::icon_link_button("#", "Remove from list", TRASH_SVG_PATH, "", theme),
        empty = super::empty_state(theme, "No history yet."),
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        fg = theme.help_fg,
        heading = theme.help_heading,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        danger = theme.help_danger,
    );
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
        let json: serde_json::Value = serde_json::from_str(&results_json(&history, "", 0)).unwrap();
        assert_eq!(json["total"], 2);
        assert_eq!(json["rows"][0]["host"], "a.test", "host without www.");
        assert_eq!(json["rows"][1]["http"], true);
        assert_eq!(json["rows"][1]["title"], serde_json::Value::Null);
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
            serde_json::from_str(&results_json(&history, "h7.test", 10)).unwrap();
        assert_eq!(json["offset"], 10);
        assert_eq!(json["total"], 34);
        assert_eq!(json["rows"].as_array().unwrap().len(), 24);
    }
}
