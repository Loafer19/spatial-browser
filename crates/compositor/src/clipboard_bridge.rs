// OSR has no OS clipboard surface. Copy: inject COPY_BRIDGE_SCRIPT on each
// load → clipboard://copy → wl-copy. Paste: hotkeys intercept Ctrl+V, wl-paste,
// then execCommand('insertText') (fires real paste events; works in contenteditable).
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

/// JS single-quoted literal for execute_java_script (no HTML escape — raw JS).
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
