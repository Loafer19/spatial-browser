// Ctrl+T omnibox (`omnibox://go` → PENDING_OMNIBOX).

use super::{html_escape, js_string_literal};
use crate::output::Theme;

/// Separate script after SCRIPT to set DEFAULT_ENGINE (keeps SCRIPT a plain const).
fn default_engine_override(default_engine: &str) -> String {
    format!(
        "<script>DEFAULT_ENGINE = {};</script>",
        js_string_literal(default_engine)
    )
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
// Suggestions: typed text → `omnibox://suggest` → Rust → omniSuggest().
let suggestions = [], selected = -1, lastQuery = '', suggestTimer = null;
function askSuggest(value) {
  clearTimeout(suggestTimer);
  selected = -1;
  suggestTimer = setTimeout(() => {
    lastQuery = value;
    if (!value.trim()) { omniSuggest({ query: value, rows: [] }); return; }
    location = 'omnibox://suggest?q=' + encodeURIComponent(value);
  }, 80);
}
function omniSuggest(res) {
  if (res.query !== lastQuery) return; // stale
  suggestions = res.rows;
  selected = -1;
  renderSuggest();
}
function renderSuggest() {
  const box = document.getElementById('suggest');
  box.textContent = '';
  suggestions.forEach((s, i) => {
    const row = document.createElement('div');
    row.className = 'sg' + (i === selected ? ' sg-active' : '');
    const main = document.createElement('span');
    main.className = 'sg-text';
    main.textContent = s.text;
    row.appendChild(main);
    if (s.title) {
      const t = document.createElement('span');
      t.className = 'sg-title';
      t.textContent = s.title;
      row.appendChild(t);
    }
    if (s.typed) {
      const t = document.createElement('span');
      t.className = 'sg-title';
      t.textContent = 'typed';
      row.appendChild(t);
    }
    row.onmousedown = e => { e.preventDefault(); go(s.text); };
    box.appendChild(row);
  });
}
function omniKey(event) {
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    if (!suggestions.length) return;
    event.preventDefault();
    const n = suggestions.length;
    // -1 = the typed text itself.
    selected = event.key === 'ArrowDown'
      ? (selected + 1 >= n ? -1 : selected + 1)
      : (selected - 1 < -1 ? n - 1 : selected - 1);
    renderSuggest();
  } else if (event.key === 'Escape' && selected >= 0) {
    event.preventDefault();
    selected = -1;
    renderSuggest();
  }
}
function submit() {
  const q = document.getElementById('q');
  go(selected >= 0 && suggestions[selected] ? suggestions[selected].text : q.value);
}
function go(raw) {
  if (!raw) return;
  const dest = resolve(raw);
  window.location = 'omnibox://go?q=' + encodeURIComponent(raw) + '&url=' + encodeURIComponent(dest);
}
</script>"#;

/// Omnibox page URL (URL/search, @prefix engines, recent chips).
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

    let html = format!(
        "{script}{engine_override}\
         <body style=\"margin:0;padding:64px 48px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <style>.sg{{display:flex;gap:12px;align-items:baseline;padding:8px 16px;\
         border-radius:6px;cursor:pointer;overflow:hidden;white-space:nowrap}}\
         .sg:hover,.sg-active{{background:{key_bg};color:{key_fg}}}\
         .sg-text{{overflow:hidden;text-overflow:ellipsis}}\
         .sg-title{{opacity:0.6;font-size:13px;overflow:hidden;text-overflow:ellipsis}}</style>\
         <form onsubmit=\"submit();return false\">\
         <input id=\"q\" autofocus autocomplete=\"off\" \
         oninput=\"askSuggest(this.value)\" onkeydown=\"omniKey(event)\" \
         placeholder=\"Search or type a URL &mdash; try @y, @ddg, @wiki, @bing...\" \
         style=\"width:100%;box-sizing:border-box;background:{card_bg};color:{fg};\
         border:1px solid {card_border};border-radius:8px;padding:14px 16px;\
         font:inherit;font-size:18px\">\
         </form>\
         <div id=\"suggest\" style=\"margin-top:6px\"></div>\
         <div style=\"margin-top:20px\">{chips}</div>\
         </body>",
        script = SCRIPT,
        engine_override = default_engine_override(default_search_engine),
        bg = theme.help_bg,
        fg = theme.help_fg,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    );
    super::serve(super::kind::OMNIBOX, html)
}
