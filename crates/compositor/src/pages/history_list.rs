// Ctrl+H history list. Group/time formatting is client-side (local TZ via JS Date;
// avoids Rust timezone crates). `history://…` → PENDING_HISTORY_ACTION.

use super::{html_escape, LIST_NAV_SCRIPT, TRASH_SVG_PATH};
use crate::output::Theme;
use crate::persistence::bookmarks::host_of;
use crate::persistence::history::HistoryEntry;

/// Plain const; local-time timestamps + client-side regrouping.
const SCRIPT: &str = r#"<script>
function pad(n) { return String(n).padStart(2, '0'); }

function initTimestamps() {
  document.querySelectorAll('.hist-row').forEach(row => {
    const d = new Date(parseInt(row.dataset.timestamp, 10) * 1000);
    const dateStr = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    const timeStr = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
    row.dataset.date = dateStr;
    row.querySelector('.hist-date').textContent = dateStr;
    row.querySelector('.hist-time').textContent = timeStr;
  });
}

let groupMode = 'flat';

function applyGrouping(mode) {
  groupMode = mode;
  const container = document.getElementById('rows');
  // Filtered-out rows go last and get no headers.
  const all = Array.from(container.querySelectorAll('.hist-row'));
  const rows = all.filter(r => !r.hidden);
  const hidden = all.filter(r => r.hidden);
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
    // Rows are already most-recent-first, so a date change in that
    // order is exactly a day boundary — no re-sorting needed, just
    // insert a header wherever the local date changes.
    let last = null;
    rows.forEach(r => {
      if (r.dataset.date !== last) { addHeader(r.dataset.date); last = r.dataset.date; }
      container.appendChild(r);
    });
  } else if (mode === 'host') {
    // The order hosts are first seen in is "most recently visited host
    // first"; each group's rows keep their relative chronological order.
    const order = [];
    rows.forEach(r => { if (!order.includes(r.dataset.host)) order.push(r.dataset.host); });
    order.forEach(host => {
      addHeader(host);
      rows.filter(r => r.dataset.host === host).forEach(r => container.appendChild(r));
    });
  }
  hidden.forEach(r => container.appendChild(r));
  const none = document.getElementById('no-match');
  if (none) none.hidden = rows.length > 0 || all.length === 0;
}

// Search: every word must appear in the title or URL.
function filterRows(query) {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  document.querySelectorAll('.hist-row').forEach(r => {
    r.hidden = !words.every(w => r.dataset.search.includes(w));
  });
  applyGrouping(groupMode);
  setListActive(0);
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
</script>"#;

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

/// Builds the history-list page's spatial:// URL. `pub(crate)` so app.rs
/// can rebuild this page in place after a remove/clear (same
/// close+respawn pattern as the bookmarks list — see
/// refresh_bookmarks_page).
pub(crate) fn page_url(theme: &Theme, history: &[HistoryEntry]) -> String {
    let mut rows = String::new();
    if history.is_empty() {
        rows.push_str(&super::empty_state(theme, "No history yet."));
    }

    for (index, entry) in history.iter().enumerate() {
        let host = host_of(&entry.url);
        let short = short_url(&entry.url);
        let letter = short
            .host
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        let title = entry.title.as_deref().filter(|t| !t.is_empty());
        let search = format!("{} {}", title.unwrap_or(""), entry.url).to_lowercase();
        let badge = if short.insecure {
            "<span class=\"hist-http\" title=\"Not encrypted (http)\">http</span>"
        } else {
            ""
        };
        let short_html = format!(
            "{badge}<span class=\"hist-host\">{host}</span><span class=\"hist-rest\">{rest}</span>",
            host = html_escape(short.host),
            rest = html_escape(short.rest),
        );
        // Title on top when known, short URL under it; else the URL alone.
        let text = match title {
            Some(title) => format!(
                "<span class=\"hist-title\">{}</span><span class=\"hist-url\">{short_html}</span>",
                html_escape(title)
            ),
            None => format!("<span class=\"hist-title\">{short_html}</span>"),
        };
        rows.push_str(&format!(
            "<div class=\"hist-row list-row\" data-timestamp=\"{timestamp}\" data-host=\"{host_attr}\" \
             data-search=\"{search_attr}\" title=\"{url_attr}\" \
             data-open=\"history://open/{index}\" onclick=\"location='history://open/{index}'\" \
             style=\"display:flex;align-items:center;gap:10px;padding:8px 14px;\
             cursor:pointer;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             {favicon}\
             <span class=\"hist-text\">{text}</span>\
             <span class=\"hist-date\" style=\"flex-shrink:0;color:{fg};opacity:0.6;\
             font-size:12px;white-space:nowrap\"></span>\
             <span class=\"hist-time\" style=\"flex-shrink:0;color:{fg};opacity:0.6;\
             font-size:12px;white-space:nowrap\"></span>\
             {remove_button}</div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            favicon = super::favicon_tile(host, &letter, theme),
            fg = theme.help_fg,
            timestamp = entry.visited_at,
            host_attr = html_escape(short.host),
            search_attr = html_escape(&search),
            url_attr = html_escape(&entry.url),
            remove_button = super::icon_link_button(
                &format!("history://remove/{index}"),
                "Remove from list",
                TRASH_SVG_PATH,
                "onclick=\"event.stopPropagation()\"",
                theme,
            ),
        ));
    }

    let html = format!(
        "{script}{nav_script}\
         <style>\
         .hist-row:hover,.hist-row.list-active{{background:{key_bg}!important}}\
         .hist-row:hover span,.hist-row.list-active span{{color:{key_fg}!important}}\
         {icon_hover}\
         .hist-header{{margin:16px 0 0;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;opacity:0.8;color:{heading}}}\
         .hist-header:first-child{{margin-top:0}}\
         #rows.mode-date .hist-date{{display:none}}\
         .hist-row[hidden]{{display:none!important}}\
         .hist-text{{flex:1;min-width:0;display:flex;flex-direction:column;gap:2px}}\
         .hist-title,.hist-url{{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:{fg}}}\
         .hist-url{{font-size:12px;opacity:0.65}}\
         .hist-rest{{opacity:0.55}}\
         .hist-http{{font-size:10px;padding:1px 5px;margin-right:6px;border-radius:4px;\
         border:1px solid {danger};color:{danger}!important;vertical-align:1px}}\
         #q{{width:100%;box-sizing:border-box;background:{card_bg};color:{fg};\
         border:1px solid {card_border};border-radius:8px;padding:8px 12px;font:inherit;\
         margin-bottom:12px}}\
         .group-toggle button{{background:{card_bg};color:{fg};opacity:0.7;\
         border:1px solid {card_border};border-radius:6px;padding:5px 12px;\
         font:inherit;font-size:13px;cursor:pointer}}\
         .group-toggle button.active{{background:{key_bg};color:{key_fg};opacity:1;\
         border-color:{key_bg}}}\
         </style>\
         {body_open}\
         <div style=\"display:flex;align-items:baseline;justify-content:space-between;\
         margin:0 0 16px\">\
         <h1 style=\"margin:0;color:{heading};font-size:20px\">History</h1>\
         <a href=\"history://clear\" style=\"color:{fg};opacity:0.6;font-size:13px\">Clear all</a>\
         </div>\
         <input id=\"q\" type=\"search\" placeholder=\"Search history (title or URL)\" autofocus \
         oninput=\"filterRows(this.value)\" onkeydown=\"searchKey(event)\">\
         <div class=\"group-toggle\" style=\"display:flex;gap:6px;margin-bottom:16px\">\
         <button data-mode=\"flat\" onclick=\"applyGrouping('flat')\" class=\"active\">Recent</button>\
         <button data-mode=\"date\" onclick=\"applyGrouping('date')\">By day</button>\
         <button data-mode=\"host\" onclick=\"applyGrouping('host')\">By site</button>\
         </div>\
         <div id=\"rows\" \
         style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div>\
         <p id=\"no-match\" hidden style=\"opacity:0.6\">No matches.</p></body>",
        script = SCRIPT,
        nav_script = LIST_NAV_SCRIPT,
        icon_hover = super::icon_button_hover_css(theme),
        body_open = super::body_open(theme, "onload=\"initTimestamps()\""),
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
    fn rows_show_title_and_escape_everything() {
        let history = vec![
            HistoryEntry {
                url: "https://www.a.test/x\"><b>".into(),
                visited_at: 1,
                title: Some("<script>alert(1)</script>".into()),
            },
            HistoryEntry {
                url: "http://b.test/".into(),
                visited_at: 2,
                title: None,
            },
        ];
        let html = served_html(&page_url(&THEMES[0], &history));
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!html.contains("x\"><b>"));
        assert!(html.contains(">a.test<"), "host without www.");
        assert!(
            html.contains("class=\"hist-http\""),
            "http badge for b.test"
        );
        assert!(!html.contains(">https://"), "no scheme shown");
    }
}
