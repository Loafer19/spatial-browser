// Generated `data:` utility pages shown on the canvas — F1 help, the
// bookmarks list, and the new-page omnibox. Kept separate from
// hotkeys.rs (which only dispatches canvas-level keyboard shortcuts) so
// that file doesn't end up half HTML/CSS/JS generation, half input
// handling.

pub mod bookmarks_list;
pub mod downloads_list;
pub mod help;
pub mod history_list;
pub mod omnibox;
pub mod switcher;

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
