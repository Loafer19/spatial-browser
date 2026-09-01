// Generated data: chrome pages. Always use `data:text/html;charset=utf-8,`
// (no charset → Latin-1 mojibake) and escape `#` (otherwise starts a fragment).

use crate::output::Theme;

pub mod bookmarks_list;
pub mod context_menu;
pub mod downloads_list;
pub mod help;
pub mod history_list;
pub mod omnibox;
pub mod passwords_list;
pub mod settings_list;
pub mod switcher;
pub mod userscripts_list;
pub mod workspace_list;

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// JS single-quoted literal safe inside a double-quoted HTML attribute.
pub(crate) fn js_string_literal(s: &str) -> String {
    let js_escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{}'", html_escape(&js_escaped))
}

/// ArrowUp/Down/Enter for `.list-row[data-open]`; skips focused `<input>`.
pub(crate) const LIST_NAV_SCRIPT: &str = r#"<script>
let listActiveIndex = 0;
function listRows() {
  return Array.from(document.querySelectorAll('.list-row')).filter(r => r.offsetParent !== null);
}
function setListActive(index) {
  const rows = listRows();
  if (!rows.length) return;
  listActiveIndex = ((index % rows.length) + rows.length) % rows.length;
  rows.forEach(r => r.classList.remove('list-active'));
  rows[listActiveIndex].classList.add('list-active');
  rows[listActiveIndex].scrollIntoView({block: 'nearest'});
}
document.addEventListener('keydown', (event) => {
  if (event.target.tagName === 'INPUT') return;
  if (event.key === 'ArrowDown') {
    event.preventDefault();
    setListActive(listActiveIndex + 1);
  } else if (event.key === 'ArrowUp') {
    event.preventDefault();
    setListActive(listActiveIndex - 1);
  } else if (event.key === 'Enter') {
    const row = listRows()[listActiveIndex];
    if (row && row.dataset.open) window.location = row.dataset.open;
  }
});
document.addEventListener('mouseover', (event) => {
  const row = event.target.closest('.list-row');
  if (!row) return;
  const index = listRows().indexOf(row);
  if (index >= 0) setListActive(index);
});
window.addEventListener('load', () => setListActive(0));
</script>"#;

pub(crate) fn body_open(theme: &Theme, extra_attrs: &str) -> String {
    format!(
        "<body {extra_attrs} style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">",
        bg = theme.help_bg,
        fg = theme.help_fg,
    )
}

/// Empty-list placeholder (`text` is app-authored markup, not escaped).
pub(crate) fn empty_state(theme: &Theme, text: &str) -> String {
    format!(
        "<p style=\"color:{fg};opacity:0.7\">{text}</p>",
        fg = theme.help_fg
    )
}

/// Favicon tile with letter fallback (`letter` already uppercased).
pub(crate) fn favicon_tile(host: &str, letter: &str, theme: &Theme) -> String {
    format!(
        "<span style=\"position:relative;width:20px;height:20px;flex-shrink:0\">\
         <span style=\"position:absolute;inset:0;border-radius:4px;background:{key_bg};\
         color:{key_fg};display:flex;align-items:center;justify-content:center;\
         font-size:11px;font-weight:700\">{letter}</span>\
         <img src=\"https://{host}/favicon.ico\" width=\"20\" height=\"20\" \
         style=\"position:absolute;inset:0;border-radius:4px\" \
         onerror=\"this.style.display='none'\"></span>",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    )
}

pub(crate) const TRASH_SVG_PATH: &str =
    "M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z";

pub(crate) const CHECKMARK_SVG_PATH: &str = "M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z";

/// Per-row icon `<a href>` (`extra_attrs` e.g. stopPropagation on delete).
pub(crate) fn icon_link_button(
    href: &str,
    title: &str,
    svg_path: &str,
    extra_attrs: &str,
    theme: &Theme,
) -> String {
    format!(
        "<a href=\"{href}\" title=\"{title}\" class=\"bm-icon-btn\" {extra_attrs} \
         style=\"flex-shrink:0;margin-left:4px;display:flex;align-items:center;\
         justify-content:center;width:26px;height:26px;border-radius:6px;\
         background:{bg};color:{fg};opacity:0.7;text-decoration:none\">\
         <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
         <path d=\"{svg_path}\"/></svg></a>",
        bg = theme.help_bg,
        fg = theme.help_fg,
    )
}

/// Like `icon_link_button`, but a form submit (rename-save).
pub(crate) fn icon_submit_button(title: &str, svg_path: &str, theme: &Theme) -> String {
    format!(
        "<button type=\"submit\" title=\"{title}\" class=\"bm-icon-btn\" style=\"flex-shrink:0;\
         margin-left:4px;display:flex;align-items:center;justify-content:center;width:26px;\
         height:26px;border-radius:6px;background:{bg};color:{fg};opacity:0.7;border:none;\
         cursor:pointer;font:inherit\">\
         <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
         <path d=\"{svg_path}\"/></svg></button>",
        bg = theme.help_bg,
        fg = theme.help_fg,
    )
}

/// `.bm-icon-btn:hover` rule body for splicing into page `<style>`.
pub(crate) fn icon_button_hover_css(theme: &Theme) -> String {
    format!(
        ".bm-icon-btn:hover{{background:{key_bg}!important;color:{key_fg}!important;\
         opacity:1!important}}",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    )
}
