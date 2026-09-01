// Ctrl+Shift+R reader mode: heuristic main-content extract (max <p> length).
// Toggle off reloads — DOM rewrite is not reversible in place.

pub struct ReaderTheme {
    pub name: &'static str,
    pub bg: &'static str,
    pub fg: &'static str,
    pub accent: &'static str,
}

pub const READER_THEMES: &[ReaderTheme] = &[
    ReaderTheme {
        name: "Light",
        bg: "#ffffff",
        fg: "#1a1a1a",
        accent: "#1a73e8",
    },
    ReaderTheme {
        name: "Sepia",
        bg: "#f4ecd8",
        fg: "#3b3629",
        accent: "#8a5a2b",
    },
    ReaderTheme {
        name: "Dark",
        bg: "#1a1a1a",
        fg: "#d8d8d8",
        accent: "#7aa2f7",
    },
];

/// Extract+rewrite script. Strip scripts from a clone before document.write.
pub fn extract_script(theme: &ReaderTheme) -> String {
    format!(
        r#"
(function() {{
  function textLen(el) {{ return (el.innerText || '').length; }}
  function score(el) {{
    var ps = el.querySelectorAll('p');
    var total = 0;
    for (var i = 0; i < ps.length; i++) total += textLen(ps[i]);
    return total;
  }}
  var candidates = document.querySelectorAll('article, main, [role="main"], div, section');
  var best = document.body, bestScore = 0;
  for (var i = 0; i < candidates.length; i++) {{
    var s = score(candidates[i]);
    if (s > bestScore) {{ bestScore = s; best = candidates[i]; }}
  }}
  var clone = best.cloneNode(true);
  var junk = clone.querySelectorAll('script, style, noscript, iframe');
  for (var j = 0; j < junk.length; j++) junk[j].remove();

  var title = document.title || '';
  var escTitle = title.replace(/&/g, '&amp;').replace(/</g, '&lt;');
  var contentHtml = clone.innerHTML;

  document.open();
  document.write(
    '<!doctype html><html><head><meta charset="utf-8"><title>' + escTitle + '</title>' +
    '<style>' +
    'html,body{{margin:0;padding:0;background:{bg};color:{fg};overflow-x:hidden}}' +
    '.spatial-reader{{max-width:680px;margin:0 auto;padding:48px 24px 96px;' +
      'font-family:Georgia,\'Times New Roman\',serif;font-size:19px;line-height:1.7;' +
      'word-wrap:break-word;overflow-x:hidden}}' +
    // Override inline width/absolute leftovers from the source page.
    '.spatial-reader *{{max-width:100%!important;box-sizing:border-box;' +
      'position:static!important;float:none!important}}' +
    '.spatial-reader img,.spatial-reader video,.spatial-reader svg,' +
      '.spatial-reader picture,.spatial-reader canvas{{height:auto!important}}' +
    '.spatial-reader h1{{font-size:30px;line-height:1.3;margin:0 0 28px}}' +
    '.spatial-reader a{{color:{accent}}}' +
    '</style></head><body><div class="spatial-reader"><h1>' + escTitle + '</h1>' +
    contentHtml + '</div></body></html>'
  );
  document.close();
}})();
"#,
        bg = theme.bg,
        fg = theme.fg,
        accent = theme.accent,
    )
}
