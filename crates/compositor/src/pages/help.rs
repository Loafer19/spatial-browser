// The F1 help page: a static list of canvas-level shortcuts, styled to
// match whichever theme is active.

use crate::output::Theme;

pub const HELP_ENTRIES: &[(&str, &str)] = &[
    ("Ctrl+T", "New page (search or URL)"),
    ("Ctrl+Shift+T", "Reopen closed page"),
    ("Ctrl+W", "Close page"),
    ("Ctrl+R", "Reload page"),
    ("Ctrl+D", "Bookmark page"),
    ("Ctrl+B", "Bookmarks list"),
    ("Ctrl+G", "Auto-layout pages into a grid"),
    ("Ctrl+K", "Switch to an open page"),
    ("Ctrl+Tab", "Next page"),
    ("Ctrl+Shift+Tab", "Previous page"),
    ("Ctrl+Space", "Zoom to canvas"),
    ("Ctrl+= / Ctrl+-", "Zoom in / out"),
    ("Ctrl+0", "Reset zoom"),
    ("Alt+Left/Right", "Back / forward"),
    ("Alt+Left-drag", "Move a page"),
    ("Drag corner", "Resize a page"),
    ("Middle-drag", "Pan canvas"),
    ("Shift+Left-drag", "Pan canvas (trackpad)"),
    ("Ctrl+Scroll", "Zoom canvas"),
    ("Ctrl+Shift+0", "Reset canvas view"),
    ("Ctrl+Shift+Space", "Cycle UI theme"),
    ("F1", "This page"),
];

/// Builds the F1 help page's `data:` URL from `theme`'s palette, so it's
/// never out of sync with what's actually on screen. Built at runtime
/// (not a `concat!`-based const like the fixed entry list could be)
/// because the theme is chosen at runtime.
pub fn page_url(theme: &Theme) -> String {
    let mut rows = String::new();
    for (key, desc) in HELP_ENTRIES {
        rows.push_str(&format!(
            "<div style=\"display:flex;justify-content:space-between;align-items:center;\
             gap:16px;padding:10px 14px;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             <kbd style=\"flex-shrink:0;white-space:nowrap;background:{key_bg};color:{key_fg};\
             padding:4px 10px;border-radius:6px;font-weight:600;font-size:13px\">{key}</kbd>\
             <span style=\"text-align:right;white-space:nowrap\">{desc}</span></div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            key_bg = theme.help_key_bg,
            key_fg = theme.help_key_fg,
        ));
    }
    // Hex colors avoided deliberately: an unescaped `#` in a `data:` URL
    // starts a fragment, silently truncating everything after it from
    // the actual document — every Theme field here is an rgb() string.
    format!(
        "data:text/html,<body style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <h1 style=\"margin:0 0 20px;color:{heading};font-size:20px\">\
         spatial-browser &mdash; shortcuts ({name})</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        bg = theme.help_bg,
        fg = theme.help_fg,
        heading = theme.help_heading,
        name = theme.name,
    )
}
