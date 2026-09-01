// Ctrl+K switcher — filter open pages (`switcher://go` → PENDING_SWITCH).

use super::html_escape;
use crate::output::Theme;
use crate::persistence::bookmarks::host_of;

/// Plain const (not format!) so JS `{`/`}` need no escaping.
const SCRIPT: &str = r#"<script>
let activeIndex = 0;
function visibleRows() {
  return Array.from(document.querySelectorAll('.switch-row')).filter(r => r.style.display !== 'none');
}
function setActive(index) {
  const rows = visibleRows();
  if (!rows.length) return;
  activeIndex = ((index % rows.length) + rows.length) % rows.length;
  rows.forEach(r => r.classList.remove('switch-active'));
  rows[activeIndex].classList.add('switch-active');
  rows[activeIndex].scrollIntoView({block: 'nearest'});
}
function filterRows() {
  const q = document.getElementById('q').value.toLowerCase();
  document.querySelectorAll('.switch-row').forEach(row => {
    row.style.display = row.dataset.search.includes(q) ? 'flex' : 'none';
  });
  setActive(0);
}
function setActiveRow(row) {
  const index = visibleRows().indexOf(row);
  if (index >= 0) setActive(index);
}
function go(id) {
  window.location = 'switcher://go/' + id;
}
function submitActive() {
  const active = document.querySelector('.switch-active');
  if (active) go(active.dataset.id);
}
function handleKey(event) {
  if (event.key === 'Enter') {
    submitActive();
  } else if (event.key === 'ArrowDown') {
    event.preventDefault();
    setActive(activeIndex + 1);
  } else if (event.key === 'ArrowUp') {
    event.preventDefault();
    setActive(activeIndex - 1);
  }
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
        rows.push_str(&super::empty_state(theme, "No other open pages."));
    }

    for (id, url) in entries {
        let host = host_of(url);
        let letter = host
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        let search = html_escape(&url.to_lowercase());
        rows.push_str(&format!(
            "<div class=\"switch-row\" data-search=\"{search}\" data-id=\"{id}\" \
             onclick=\"go({id})\" onmouseenter=\"setActiveRow(this)\" \
             style=\"display:flex;align-items:center;gap:10px;\
             padding:10px 14px;cursor:pointer;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             {favicon}\
             <span style=\"overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
             flex:1;color:{fg}\">{label}</span></div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            favicon = super::favicon_tile(host, &letter, theme),
            fg = theme.help_fg,
            label = html_escape(url),
        ));
    }

    format!(
        "data:text/html;charset=utf-8,{script}\
         <style>.switch-row:hover,.switch-active{{background:{key_bg}!important}}\
         .switch-row:hover span,.switch-active span{{color:{key_fg}!important}}</style>\
         {body_open}\
         <input id=\"q\" autofocus placeholder=\"Switch to a page...\" oninput=\"filterRows()\" \
         onkeydown=\"handleKey(event)\" \
         style=\"width:100%;box-sizing:border-box;background:{card_bg};color:{fg};\
         border:1px solid {card_border};border-radius:8px;padding:10px 14px;\
         font:inherit;font-size:16px;margin-bottom:16px\">\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        script = SCRIPT,
        body_open = super::body_open(theme, "onload=\"filterRows()\""),
        fg = theme.help_fg,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    )
}
