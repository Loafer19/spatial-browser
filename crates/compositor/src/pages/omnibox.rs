// Ctrl+T omnibox (`omnibox://go` → PENDING_OMNIBOX).

use super::{html_escape, js_string_literal};
use crate::output::Theme;

/// Separate script after SCRIPT to set DEFAULT_ENGINE (keeps SCRIPT a plain const).
fn default_engine_override(default_engine: &str) -> String {
    let js_escaped = default_engine.replace('\\', "\\\\").replace('\'', "\\'");
    format!("<script>DEFAULT_ENGINE = '{js_escaped}';</script>")
}

/// Plain const (not format!) so JS braces need no `{{` escaping.
const SCRIPT: &str = r#"<script>
const PREFIXES = {
  '@g': 'https://www.google.com/search?q=',
  '@y': 'https://www.youtube.com/results?search_query=',
  '@ddg': 'https://duckduckgo.com/?q=',
  '@bing': 'https://www.bing.com/search?q=',
  '@wiki': 'https://en.wikipedia.org/w/index.php?search='
};
// `var`: a following script reassigns DEFAULT_ENGINE from Settings.
var DEFAULT_ENGINE = 'https://www.google.com/search?q=';
function resolve(raw) {
  const trimmed = raw.trim();
  // Prefix lookup is case-insensitive (`@G rust` works same as `@g
  // rust`) — PREFIXES' own keys are already all-lowercase, so only the
  // matched prefix needs lowercasing before the lookup.
  const m = trimmed.match(/^(@[a-zA-Z]+)\s+(.*)$/);
  if (m && PREFIXES[m[1].toLowerCase()]) return PREFIXES[m[1].toLowerCase()] + encodeURIComponent(m[2]);
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(trimmed)) return trimmed;
  if (/^[^\s]+\.[^\s]+$/.test(trimmed)) return 'https://' + trimmed;
  return DEFAULT_ENGINE + encodeURIComponent(trimmed);
}
function go(raw) {
  if (!raw) return;
  const dest = resolve(raw);
  window.location = 'omnibox://go?q=' + encodeURIComponent(raw) + '&url=' + encodeURIComponent(dest);
}
</script>"#;

/// Omnibox `data:` URL (URL/search, @prefix engines, recent chips).
pub fn page_url(theme: &Theme, typed_history: &[String], default_search_engine: &str) -> String {
    let mut chips = String::new();
    for entry in typed_history.iter().take(10) {
        chips.push_str(&format!(
            "<button onclick=\"go({entry_js})\" style=\"background:{card_bg};color:{fg};\
             border:1px solid {card_border};border-radius:6px;padding:6px 10px;\
             font:inherit;font-size:13px;cursor:pointer;margin:0 6px 6px 0\">{entry_html}</button>",
            entry_js = js_string_literal(entry),
            entry_html = html_escape(entry),
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            fg = theme.help_fg,
        ));
    }

    format!(
        "data:text/html;charset=utf-8,{script}{engine_override}\
         <body style=\"margin:0;padding:64px 48px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <form onsubmit=\"go(document.getElementById('q').value);return false\">\
         <input id=\"q\" autofocus \
         placeholder=\"Search or type a URL &mdash; try @y, @ddg, @wiki, @bing...\" \
         style=\"width:100%;box-sizing:border-box;background:{card_bg};color:{fg};\
         border:1px solid {card_border};border-radius:8px;padding:14px 16px;\
         font:inherit;font-size:18px\">\
         </form>\
         <div style=\"margin-top:20px\">{chips}</div>\
         </body>",
        script = SCRIPT,
        engine_override = default_engine_override(default_search_engine),
        bg = theme.help_bg,
        fg = theme.help_fg,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
    )
}
