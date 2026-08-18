// The Ctrl+K page switcher: a filterable list of open pages, typed
// search rather than positional (Ctrl+1/2/3-style) selection — a
// page's position in the list is its z-order, which changes on every
// click (Session::bring_to_front), so a stable "page 3" the way a
// browser's tab bar has one doesn't exist here. See cef-bridge's
// OsrRequestHandler / app.rs's PENDING_SWITCH for how a row click or
// Enter here gets handled.

use super::html_escape;
use crate::output::Theme;
use crate::persistence::bookmarks::host_of;

/// Not run through `format!` (see omnibox.rs's SCRIPT for why) — plain
/// substring filtering, keeping the first visible row "active" so
/// Enter with no further interaction jumps to the top match (or the
/// most recently opened page, if the query is still empty).
const SCRIPT: &str = r#"<script>
function filterRows() {
  const q = document.getElementById('q').value.toLowerCase();
  const rows = document.querySelectorAll('.switch-row');
  let first = null;
  rows.forEach(row => {
    const match = row.dataset.search.includes(q);
    row.style.display = match ? 'flex' : 'none';
    row.classList.remove('switch-active');
    if (match && !first) first = row;
  });
  if (first) first.classList.add('switch-active');
}
function go(id) {
  window.location = 'switcher://go/' + id;
}
function submitActive() {
  const active = document.querySelector('.switch-active');
  if (active) go(active.dataset.id);
}
</script>"#;

/// Builds the switcher page's `data:` URL — one row per currently open,
/// non-ephemeral page (`id` is CEF's own per-browser identifier, `url`
/// its current URL). Ephemeral pages (F1 help, bookmarks list, this
/// switcher itself) are excluded by the caller (hotkeys::open_switcher):
/// transient utility pages aren't something worth switching *to*.
pub fn page_url(theme: &Theme, entries: &[(i32, String)]) -> String {
    let mut rows = String::new();
    if entries.is_empty() {
        rows.push_str(&format!(
            "<p style=\"color:{fg};opacity:0.7\">No other open pages.</p>",
            fg = theme.help_fg,
        ));
    }

    for (id, url) in entries {
        let host = host_of(url);
        let letter = host.chars().next().unwrap_or('?').to_uppercase();
        let search = html_escape(&url.to_lowercase());
        rows.push_str(&format!(
            "<div class=\"switch-row\" data-search=\"{search}\" data-id=\"{id}\" \
             onclick=\"go({id})\" style=\"display:flex;align-items:center;gap:10px;\
             padding:10px 14px;cursor:pointer;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             <span style=\"position:relative;width:20px;height:20px;flex-shrink:0\">\
             <span style=\"position:absolute;inset:0;border-radius:4px;background:{key_bg};\
             color:{key_fg};display:flex;align-items:center;justify-content:center;\
             font-size:11px;font-weight:700\">{letter}</span>\
             <img src=\"https://{host}/favicon.ico\" width=\"20\" height=\"20\" \
             style=\"position:absolute;inset:0;border-radius:4px\" \
             onerror=\"this.style.display='none'\"></span>\
             <span style=\"overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
             flex:1;color:{fg}\">{label}</span></div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            key_bg = theme.help_key_bg,
            key_fg = theme.help_key_fg,
            fg = theme.help_fg,
            label = html_escape(url),
        ));
    }

    format!(
        "data:text/html,{script}\
         <style>.switch-row:hover,.switch-active{{background:{key_bg}!important}}\
         .switch-row:hover span,.switch-active span{{color:{key_fg}!important}}</style>\
         <body onload=\"filterRows()\" style=\"margin:0;padding:32px;background:{bg};\
         color:{fg};font-family:ui-monospace,monospace;font-size:15px\">\
         <input id=\"q\" autofocus placeholder=\"Switch to a page...\" oninput=\"filterRows()\" \
         onkeydown=\"if(event.key==='Enter'){{submitActive()}}\" \
         style=\"width:100%;box-sizing:border-box;background:{card_bg};color:{fg};\
         border:1px solid {card_border};border-radius:8px;padding:10px 14px;\
         font:inherit;font-size:16px;margin-bottom:16px\">\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        script = SCRIPT,
        bg = theme.help_bg,
        fg = theme.help_fg,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    )
}
