// The F1 help page: a static list of canvas-level shortcuts, grouped
// by what they act on (mirrors hotkeys.rs's own grouping), styled to
// match whichever theme is active.

use crate::output::Theme;

pub const HELP_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Pages",
        &[
            ("Ctrl+T", "New page (search or URL)"),
            ("Ctrl+Shift+T", "Reopen closed page"),
            ("Ctrl+W", "Close page"),
            ("Ctrl+R", "Reload page"),
            ("Ctrl+V", "Paste from clipboard"),
            ("Ctrl+Shift+C", "Copy page URL"),
            ("Ctrl+Shift+R", "Toggle reader mode"),
            ("Alt+Left/Right", "Back / forward"),
            ("Ctrl+= / Ctrl+-", "Zoom in / out"),
            ("Ctrl+0", "Reset zoom"),
        ],
    ),
    (
        "Lists",
        &[
            ("Ctrl+D", "Bookmark page"),
            ("Ctrl+B", "Bookmarks list"),
            ("Ctrl+J", "Downloads list"),
            ("Ctrl+H", "History list"),
            ("Ctrl+K", "Switch to an open page"),
            ("Ctrl+Shift+W", "Workspace list"),
            ("Ctrl+Shift+U", "Userscripts list"),
            ("Ctrl+,", "Settings"),
        ],
    ),
    (
        "Canvas",
        &[
            ("Ctrl+G", "Auto-layout pages into a grid"),
            ("Ctrl+Tab", "Next page"),
            ("Ctrl+Shift+Tab", "Previous page"),
            ("Alt+Left-drag", "Move a page"),
            ("Drag corner", "Resize a page"),
            ("Ctrl+Space", "Zoom to canvas"),
            ("Middle-drag", "Pan canvas"),
            ("Shift+Left-drag", "Pan canvas (trackpad)"),
            ("Ctrl+Scroll", "Zoom canvas"),
            ("One-finger drag (empty)", "Pan canvas (touch)"),
            ("Two-finger pinch/drag", "Zoom + pan canvas (touch)"),
            ("Ctrl+Shift+0", "Reset canvas view"),
        ],
    ),
    (
        "Other",
        &[
            ("Ctrl+Shift+Space", "Cycle UI theme"),
            ("F1 / Ctrl+/", "This page"),
        ],
    ),
];

/// Builds the F1 help page's `data:` URL from `theme`'s palette, so it's
/// never out of sync with what's actually on screen. Built at runtime
/// (not a `concat!`-based const like the fixed entry list could be)
/// because the theme is chosen at runtime.
pub fn page_url(theme: &Theme) -> String {
    let mut rows = String::new();
    for (group, entries) in HELP_GROUPS {
        rows.push_str(&format!(
            "<h2 style=\"margin:16px 0 0;font-size:13px;text-transform:uppercase;\
             letter-spacing:0.05em;color:{heading};opacity:0.8\">{group}</h2>",
            heading = theme.help_heading,
        ));
        for (key, desc) in *entries {
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
    }
    // Hex colors avoided deliberately: an unescaped `#` in a `data:` URL
    // starts a fragment, silently truncating everything after it from
    // the actual document — every Theme field here is an rgb() string.
    format!(
        "data:text/html;charset=utf-8,{body_open}\
         <h1 style=\"margin:0 0 4px;color:{heading};font-size:20px\">\
         spatial-browser &mdash; shortcuts ({name})</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        body_open = super::body_open(theme, ""),
        heading = theme.help_heading,
        name = theme.name,
    )
}
