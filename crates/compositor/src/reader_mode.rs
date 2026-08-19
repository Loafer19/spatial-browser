// A lightweight, heuristic reader mode: Ctrl+Shift+R (hotkeys.rs)
// extracts a page's main content — the candidate element (article/
// main/div/section) with the highest cumulative <p> text length, the
// core idea behind Mozilla's Readability algorithm without pulling in
// the full library — and replaces the document with a plain,
// single-column article view in one of a few reading themes (Settings
// page, `reader_theme`). Toggling back off (hotkeys.rs, gated on
// `browser::Page::reader_mode`) reloads the page rather than trying to
// reconstruct the original DOM: reader mode already threw away layout
// and most interactivity on the way in, so there's nothing left worth
// restoring in place — a reload is the same "give me the real page
// back" affordance the user already has for any other stuck page.

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

/// The extraction+rewrite script, parameterized by the chosen reading
/// theme's colors. Strips `<script>`/`<style>`/`<noscript>` out of a
/// *clone* of the winning candidate before reading its `innerHTML` —
/// `document.write`, used to replace the whole document further down,
/// parses and runs any `<script>` it's given, which would double-run
/// whatever a normal page load already ran once.
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
    // Extracted content brings its own inline styles along (absolute-
    // positioned leftover header/logo markup, explicit pixel
    // width/height attributes meant for the original page's much wider
    // layout) — `max-width:100%!important` on every descendant caps
    // width regardless of what inline style set it to (max-width still
    // constrains a wider `width`, inline or not — it's a separate box-
    // model property, not a cascade fight over the same one), and
    // resetting position/float keeps a stray `position:absolute` logo
    // from escaping the column instead of flowing with the text.
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
