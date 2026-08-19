// Generated `data:` utility pages shown on the canvas — F1 help, the
// bookmarks list, and the new-page omnibox. Kept separate from
// hotkeys.rs (which only dispatches canvas-level keyboard shortcuts) so
// that file doesn't end up half HTML/CSS/JS generation, half input
// handling.

use crate::output::Theme;

pub mod bookmarks_list;
pub mod downloads_list;
pub mod help;
pub mod history_list;
pub mod omnibox;
pub mod settings_list;
pub mod switcher;
pub mod workspace_list;

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A JS single-quoted string literal for `s`, safe to embed inside a
/// double-quoted HTML attribute (e.g. `onclick="go({this})"`). JS-escapes
/// first (backslash, single quote), then HTML-attribute-escapes the
/// result — the two passes don't interfere: JS escaping only introduces
/// backslashes and `\'`, neither of which `html_escape` touches.
pub(crate) fn js_string_literal(s: &str) -> String {
    let js_escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{}'", html_escape(&js_escaped))
}

/// Shared ArrowUp/ArrowDown/Enter row navigation for the bookmarks/
/// downloads/history/workspaces list pages — matching switcher.rs's own
/// row-highlight behavior so every list page's keyboard handling is
/// consistent. A page opts in by marking each row `class="list-row"`
/// with a `data-open="<url>"` attribute holding whatever its primary
/// action is (the same URL its main click handler already navigates
/// to); Enter on the highlighted row navigates there. Typing into a
/// rename `<input>` (bookmarks/workspaces) is left alone — the guard
/// below only acts when the focused element isn't one, so the input's
/// own native Enter-submits-the-form behavior is untouched.
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

// The shared "card" look every generated page uses for its body — same
// margin/padding/colors/font everywhere except omnibox.rs (bigger
// padding, no rows to align with). `extra_attrs` covers the two pages
// that need a body-level event handler (switcher's `onload`, history's
// `onload`) without a third parameter for everyone else.
pub(crate) fn body_open(theme: &Theme, extra_attrs: &str) -> String {
    format!(
        "<body {extra_attrs} style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">",
        bg = theme.help_bg,
        fg = theme.help_fg,
    )
}

/// A muted placeholder line for a list page with nothing in it yet
/// (`text` may contain markup, e.g. `&mdash;` — it's always static,
/// app-authored copy, never user data, so this doesn't escape it).
pub(crate) fn empty_state(theme: &Theme, text: &str) -> String {
    format!(
        "<p style=\"color:{fg};opacity:0.7\">{text}</p>",
        fg = theme.help_fg
    )
}

/// The favicon-with-fallback-letter tile used by every list row that
/// represents a site (bookmarks/downloads/history/switcher): a colored
/// initial-letter square underneath, the real `favicon.ico` painted over
/// it once (if) it loads. `letter` is pre-uppercased by the caller
/// (`char::to_uppercase()` returns an iterator, not a `char`, so there's
/// nothing simpler to accept here than an already-formatted string).
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

/// Trash-can SVG path shared by every list page's "remove this entry"
/// button.
pub(crate) const TRASH_SVG_PATH: &str =
    "M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z";

/// Checkmark SVG path shared by bookmarks/workspaces' inline-rename
/// "save" button.
pub(crate) const CHECKMARK_SVG_PATH: &str = "M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z";

/// The small square icon button every list row uses for a navigating
/// per-row action (delete, load, ...) — an `<a href>` through the same
/// custom-scheme interception every other list-page link uses.
/// `extra_attrs` covers the one thing that varies: a row-click-to-open
/// handler needs `onclick="event.stopPropagation()"` on its own
/// delete link so that click doesn't also open the row.
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

/// Same shape as `icon_link_button`, for the one case that's a form
/// submit instead of a navigation (bookmarks/workspaces' rename-save).
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

/// Shared `.bm-icon-btn:hover` rule every list page's `<style>` block
/// includes — a `<style>...</style>` block, not just the rule body, so
/// callers can splice it straight into their own style block via
/// string concatenation without needing to know its innards.
pub(crate) fn icon_button_hover_css(theme: &Theme) -> String {
    format!(
        ".bm-icon-btn:hover{{background:{key_bg}!important;color:{key_fg}!important;\
         opacity:1!important}}",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    )
}
