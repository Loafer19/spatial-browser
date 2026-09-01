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

function applyGrouping(mode) {
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
}
</script>"#;

/// Builds the history-list page's `data:` URL. `pub(crate)` so app.rs
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
        let letter = host
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        rows.push_str(&format!(
            "<div class=\"hist-row list-row\" data-timestamp=\"{timestamp}\" data-host=\"{host_attr}\" \
             data-open=\"history://open/{index}\" onclick=\"location='history://open/{index}'\" \
             style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             cursor:pointer;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             {favicon}\
             <span style=\"flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;\
             white-space:nowrap;color:{fg}\">{url_html}</span>\
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
            host_attr = html_escape(host),
            url_html = html_escape(&entry.url),
            remove_button = super::icon_link_button(
                &format!("history://remove/{index}"),
                "Remove from list",
                TRASH_SVG_PATH,
                "onclick=\"event.stopPropagation()\"",
                theme,
            ),
        ));
    }

    // `class="rows-container"` rather than only `id="rows"`, so the
    // mode-toggle CSS rule below can target it as `.rows-container` —
    // an `#rows` *selector* would put a literal `#` in the data: URL
    // itself, and an unescaped `#` there starts a fragment, silently
    // truncating the rest of the document (same hazard as a hex color
    // — see other pages/ files). `getElementById('rows')` in SCRIPT is
    // unaffected: that `#` would only be a problem if it appeared in
    // the URL's actual text, not inside a JS string literal.
    format!(
        "data:text/html;charset=utf-8,{script}{nav_script}\
         <style>\
         .hist-row:hover,.hist-row.list-active{{background:{key_bg}!important}}\
         .hist-row:hover span,.hist-row.list-active span{{color:{key_fg}!important}}\
         {icon_hover}\
         .hist-header{{margin:16px 0 0;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;opacity:0.8;color:{heading}}}\
         .hist-header:first-child{{margin-top:0}}\
         .rows-container.mode-date .hist-date{{display:none}}\
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
         <div class=\"group-toggle\" style=\"display:flex;gap:6px;margin-bottom:16px\">\
         <button data-mode=\"flat\" onclick=\"applyGrouping('flat')\" class=\"active\">Recent</button>\
         <button data-mode=\"date\" onclick=\"applyGrouping('date')\">By day</button>\
         <button data-mode=\"host\" onclick=\"applyGrouping('host')\">By site</button>\
         </div>\
         <div id=\"rows\" class=\"rows-container\" \
         style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
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
    )
}
