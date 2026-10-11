// Ctrl+F find bar: a small ephemeral page above the searched tile. Sends
// `find://go/...`; the compositor answers with `__found(active, count)`.
// No user data is interpolated: the query lives only in the page's input.

use super::ui::{self, html, Icon};
use crate::output::Theme;

const CSS: &str = r#"
html, body { height: 100%; overflow: hidden; }
body.compact {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 6px 0 8px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
}
.q { position: relative; flex: 1; min-width: 0; display: flex; align-items: center; }
.q > svg { position: absolute; left: 9px; width: 15px; height: 15px; color: var(--faint); pointer-events: none; }
.q input { width: 100%; height: 32px; padding-left: 30px; padding-right: 76px; }
.q.none input { border-color: var(--danger); box-shadow: 0 0 0 3px var(--danger-soft); }
#n {
  position: absolute;
  right: 9px;
  max-width: 70px;
  overflow: hidden;
  white-space: nowrap;
  color: var(--muted);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  pointer-events: none;
}
.q.none #n { color: var(--danger-ink); }
.toggle { position: relative; display: inline-flex; flex-shrink: 0; cursor: pointer; }
.toggle input { position: absolute; opacity: 0; width: 0; height: 0; }
.toggle span {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 28px;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  color: var(--muted);
  font-size: 12.5px;
  font-weight: 650;
}
.toggle:hover span { background: var(--surface-2); color: var(--fg); }
.toggle input:checked + span { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent); }
.toggle input:focus-visible + span { box-shadow: var(--ring); }
@media (max-width: 260px) { .toggle { display: none; } .q input { padding-right: 52px; } }
"#;

const SCRIPT: &str = r#"
(function () {
  var q = document.getElementById('q');
  var matchCase = document.getElementById('case');
  var count = document.getElementById('n');
  var timer = null;
  function send(dir) {
    clearTimeout(timer);
    var c = matchCase.checked ? '1' : '0';
    location = 'find://go/search?q=' + encodeURIComponent(q.value) + '&dir=' + dir + '&case=' + c;
  }
  function close() { location = 'find://go/close'; }
  q.addEventListener('input', function () {
    clearTimeout(timer);
    timer = setTimeout(function () { send('next'); }, 120);
  });
  q.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); send(e.shiftKey ? 'prev' : 'next'); }
    else if (e.key === 'Escape') { e.preventDefault(); close(); }
  });
  matchCase.addEventListener('change', function () { send('next'); q.focus(); });
  document.getElementById('prev').addEventListener('click', function () { send('prev'); q.focus(); });
  document.getElementById('next').addEventListener('click', function () { send('next'); q.focus(); });
  document.getElementById('close').addEventListener('click', close);
  window.__found = function (active, total) {
    var typed = q.value !== '';
    count.textContent = typed ? (total ? active + ' / ' + total : 'No matches') : '';
    count.parentNode.classList.toggle('none', typed && !total);
  };
})();
"#;

pub fn page_url(theme: &Theme) -> String {
    let body = html! {
        div class="q" {
            (Icon::Search)
            input id="q" type="text" autofocus placeholder="Find in page" aria-label="Find in page"
                autocomplete="off" spellcheck="false";
            span id="n" aria-live="polite" {}
        }
        label class="toggle" title="Match case" {
            input id="case" type="checkbox" aria-label="Match case";
            span { "Aa" }
        }
        button id="prev" type="button" class="icon-btn" title="Previous (Shift+Enter)" aria-label="Previous match" {
            (Icon::ChevronUp)
        }
        button id="next" type="button" class="icon-btn" title="Next (Enter)" aria-label="Next match" {
            (Icon::ChevronDown)
        }
        button id="close" type="button" class="icon-btn" title="Close (Esc)" aria-label="Close" {
            (Icon::Close)
        }
    };
    let html = ui::Doc::new(theme, "Find")
        .class("compact")
        .escape("find://go/close")
        .keys(&[("Enter", "Next match"), ("Shift+Enter", "Previous match")])
        .css(CSS)
        .script(SCRIPT)
        .no_row()
        .render(body);
    super::serve(super::kind::FIND_BAR, html)
}

#[cfg(test)]
mod tests {
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    #[test]
    fn is_a_spatial_page() {
        assert!(super::page_url(&THEMES[0]).starts_with("spatial://page/find-bar/"));
    }

    #[test]
    fn keeps_the_find_protocol_and_keys() {
        let html = served_html(&super::page_url(&THEMES[0]));
        assert!(html.contains("'find://go/search?q=' + encodeURIComponent(q.value)"));
        assert!(html.contains("send(e.shiftKey ? 'prev' : 'next')"));
        assert!(html.contains("window.__found = function (active, total)"));
        assert!(html.contains("data-escape=\"find://go/close\""));
        assert!(html.contains("id=\"case\" type=\"checkbox\""));
    }
}
