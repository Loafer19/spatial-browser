// F1 help — shortcut list styled to the active Theme.

use super::ui::{self, html};
use crate::output::Theme;

pub const HELP_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Pages",
        &[
            ("Ctrl+T", "New page (search or URL)"),
            ("Ctrl+Shift+T", "Reopen closed page"),
            ("Ctrl+W", "Close page"),
            ("Ctrl+R", "Reload page"),
            ("Ctrl+F", "Find in page (Enter / Shift+Enter, Esc)"),
            ("Ctrl+Shift+O", "Open page in system browser"),
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
            ("Ctrl+K", "Command palette"),
            ("Ctrl+Shift+W", "Workspace list"),
            ("Ctrl+N", "New workspace"),
            ("Ctrl+1–9", "Switch workspace slot"),
            ("Ctrl+Shift+1–9", "Move page to workspace slot"),
            ("Ctrl+Shift+U", "Scripts & styles list"),
            ("Ctrl+Shift+P", "Passwords / vault"),
            ("Ctrl+Shift+L", "Lock password vault"),
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
            ("Right-click", "Context menu (page / link / canvas)"),
            ("Top-edge hover", "Show workspace chips"),
            ("Ctrl+Shift+0", "Reset canvas view"),
        ],
    ),
    (
        "Other",
        &[
            ("Ctrl+Shift+Space", "Cycle UI theme"),
            ("F1 / Ctrl+/", "This page"),
            ("F3", "Performance overlay"),
        ],
    ),
];

/// F1 help page URL using `theme` palette.
pub fn page_url(theme: &Theme) -> String {
    let body = html! {
        main class="page wide" {
            (ui::head("Keyboard shortcuts", Some(html! { "Theme: " (theme.name) " · press " kbd { "?" } " for list keys" }), html! {
                (ui::search("q", "", "Filter shortcuts", Some("")))
            }))
            div class="grid" style="--cell:340px;align-items:start" {
                @for (group, entries) in HELP_GROUPS {
                    section class="card flush" data-group {
                        h2 style="margin:12px 14px 6px" { (group) }
                        @for (key, desc) in *entries {
                            div class="row" data-nav {
                                span class="main" { span class="title" { (desc) } }
                                kbd { (key) }
                            }
                        }
                    }
                }
            }
            div class="empty hidden" data-filter-empty { (ui::Icon::Search) div { "No shortcut matches." } }
        }
    };
    let html = ui::Doc::new(theme, "Shortcuts").render(body);
    super::serve(super::kind::HELP, html)
}
