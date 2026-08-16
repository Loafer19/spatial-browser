// The Ctrl+T new-page page: a single input for a URL or search query,
// `@prefix` shortcuts for specific search engines, and recent-input
// chips. See cef-bridge's OsrRequestHandler / app.rs's PENDING_OMNIBOX
// for how this page's submission gets handled.

use super::{html_escape, js_string_literal};
use crate::output::Theme;

/// Not run through `format!` (it's a plain constant, substituted whole
/// into `page_url`'s template as one value) specifically so its own
/// `{`/`}` — everywhere in ordinary JS — don't need doubling up as
/// `{{`/`}}` to survive Rust's format-string escaping.
const SCRIPT: &str = r#"<script>
const PREFIXES = {
  '@g': 'https://www.google.com/search?q=',
  '@google': 'https://www.google.com/search?q=',
  '@y': 'https://www.youtube.com/results?search_query=',
  '@youtube': 'https://www.youtube.com/results?search_query=',
  '@ddg': 'https://duckduckgo.com/?q=',
  '@duckduckgo': 'https://duckduckgo.com/?q='
};
const DEFAULT_ENGINE = 'https://www.google.com/search?q=';
function resolve(raw) {
  const trimmed = raw.trim();
  const m = trimmed.match(/^(@[a-zA-Z]+)\s+(.*)$/);
  if (m && PREFIXES[m[1]]) return PREFIXES[m[1]] + encodeURIComponent(m[2]);
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

/// Builds the omnibox (new-page) page's `data:` URL: a single input for
/// a URL or search query, `@prefix` shortcuts for specific search
/// engines (`@g`/`@google`, `@y`/`@youtube`, `@ddg`/`@duckduckgo` — bare
/// text with no prefix falls back to Google), and recent-input chips
/// below it. All resolution happens in `SCRIPT`; submitting navigates to
/// `omnibox://go?q=...&url=...`, which cef-bridge's `on_before_browse`
/// intercepts and cancels, handing the raw text and resolved destination
/// back to the compositor (app.rs's PENDING_OMNIBOX) to log and actually
/// navigate to.
pub fn page_url(theme: &Theme, history: &[String]) -> String {
    let mut chips = String::new();
    for entry in history.iter().take(10) {
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
        "data:text/html,{script}\
         <body style=\"margin:0;padding:64px 48px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <form onsubmit=\"go(document.getElementById('q').value);return false\">\
         <input id=\"q\" autofocus \
         placeholder=\"Search or type a URL &mdash; try @y, @ddg, @google...\" \
         style=\"width:100%;box-sizing:border-box;background:{card_bg};color:{fg};\
         border:1px solid {card_border};border-radius:8px;padding:14px 16px;\
         font:inherit;font-size:18px\">\
         </form>\
         <div style=\"margin-top:20px\">{chips}</div>\
         </body>",
        script = SCRIPT,
        bg = theme.help_bg,
        fg = theme.help_fg,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
    )
}
