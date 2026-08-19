// Clipboard: CEF's windowless/OSR Chromium has no real platform
// surface (no wl_surface) to claim OS clipboard ownership through, so
// unlike a normal windowed browser, Ctrl+C/Ctrl+V don't reach the
// system clipboard at all — confirmed empirically (copied text inside
// a page never showed up in `wl-paste`). Both directions are
// reimplemented here instead of relying on CEF's own clipboard
// handling.
//
// Copy: COPY_BRIDGE_SCRIPT is (re-)injected into every page on every
// load (app.rs's PENDING_VISITS handling — a full navigation wipes
// whatever a previous injection put in the page's DOM, so it can't
// just run once at spawn). It listens for the DOM 'copy' event and
// relays the current selection to Rust through the same
// fake-scheme-navigation trick every other list page uses
// (cef-bridge's navigation.rs `clipboard://copy?text=...`, which
// writes it to the real clipboard via `wl-copy`).
//
// Paste: no round-trip needed the other way. Ctrl+V is intercepted
// natively (hotkeys.rs) rather than forwarded to CEF at all — reads
// the system clipboard directly via `wl-paste`, then inserts it via
// `document.execCommand('insertText', ...)`, which (unlike setting
// `.value` directly) fires the same events a real paste would and
// works in both plain inputs/textareas and contenteditable elements.
pub const COPY_BRIDGE_SCRIPT: &str = r#"
(function() {
  if (window.__spatialClipboardBridge__) return;
  window.__spatialClipboardBridge__ = true;
  document.addEventListener('copy', function() {
    var text = window.getSelection().toString();
    if (text) {
      location = 'clipboard://copy?text=' + encodeURIComponent(text);
    }
  }, true);
})();
"#;

/// A JS single-quoted string literal for `s`, safe to embed directly in
/// a script passed to `execute_java_script` — unlike
/// `pages::js_string_literal`, this does *not* also HTML-escape: there's
/// no HTML parser in this path at all (`execute_java_script` runs raw
/// JS source, not markup it's parsed into), so HTML-escaping would
/// corrupt the pasted text (a literal `<` would paste as the four
/// characters `&lt;`).
pub fn js_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}
