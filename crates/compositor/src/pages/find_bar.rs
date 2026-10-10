// Ctrl+F find bar: a small ephemeral page above the searched tile. Sends
// `find://go/...`; the compositor answers with `__found(active, count)`.
// No user data is interpolated: the query lives only in the page's input.
// Raw data: body, so no `#` or `%` anywhere below.

use crate::output::Theme;

const SCRIPT: &str = r#"<script>
var timer = null;
function send(dir) {
  var q = document.getElementById('q').value;
  var c = document.getElementById('case').checked ? '1' : '0';
  location = 'find://go/search?q=' + encodeURIComponent(q) + '&dir=' + dir + '&case=' + c;
}
function typed() {
  clearTimeout(timer);
  timer = setTimeout(function () { send('next'); }, 120);
}
function key(e) {
  if (e.key === 'Enter') { e.preventDefault(); send(e.shiftKey ? 'prev' : 'next'); }
  if (e.key === 'Escape') { e.preventDefault(); location = 'find://go/close'; }
}
window.__found = function (active, count) {
  var n = document.getElementById('n');
  n.textContent = document.getElementById('q').value ? (count ? active + '/' + count : 'No matches') : '';
  n.style.opacity = count ? '0.8' : '0.6';
};
</script>"#;

pub fn page_url(theme: &Theme) -> String {
    let button = format!(
        "background:{card_bg};color:{fg};border:1px solid {card_border};border-radius:6px;\
         padding:4px 9px;font:inherit;cursor:pointer",
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        fg = theme.help_fg,
    );
    format!(
        "data:text/html;charset=utf-8,{script}\
         <body style=\"margin:0;padding:8px 10px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:14px;display:flex;gap:8px;\
         align-items:center;box-sizing:border-box;height:100vh\">\
         <input id=\"q\" autofocus placeholder=\"Find in page\" oninput=\"typed()\" onkeydown=\"key(event)\" \
         style=\"flex:1;min-width:0;background:{card_bg};color:{fg};border:1px solid {card_border};\
         border-radius:6px;padding:6px 10px;font:inherit\">\
         <span id=\"n\" style=\"min-width:72px;text-align:right;opacity:0.8\"></span>\
         <label title=\"Match case\" style=\"display:flex;align-items:center;gap:3px;cursor:pointer\">\
         <input id=\"case\" type=\"checkbox\" onchange=\"send('next')\">Aa</label>\
         <button title=\"Previous (Shift+Enter)\" onclick=\"send('prev')\" style=\"{button}\">&uarr;</button>\
         <button title=\"Next (Enter)\" onclick=\"send('next')\" style=\"{button}\">&darr;</button>\
         <button title=\"Close (Esc)\" onclick=\"location='find://go/close'\" style=\"{button}\">&times;</button>\
         </body>",
        script = SCRIPT,
        bg = theme.help_bg,
        fg = theme.help_fg,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
    )
}

#[cfg(test)]
mod tests {
    use crate::output::THEMES;

    #[test]
    fn raw_data_body_has_no_fragment_or_escape_chars() {
        let url = super::page_url(&THEMES[0]);
        let body = url.strip_prefix("data:text/html;charset=utf-8,").unwrap();
        assert!(!body.contains('#'), "a raw # would cut the data: URL");
        assert!(!body.contains('%'), "a raw % would be percent-decoded");
    }
}
