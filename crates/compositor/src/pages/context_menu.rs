// Right-click context menu — small ephemeral spatial:// page at the cursor.
// Items depend on hit target (canvas / page / link / image / password).

use super::ui::{self, html, Icon};
use super::url_component;
use crate::output::Theme;

#[derive(Clone, Default)]
pub struct MenuContext {
    pub on_canvas: bool,
    pub page_url: Option<String>,
    pub link: Option<String>,
    pub image: Option<String>,
    pub password_field: bool,
    pub target_browser_id: Option<i32>,
}

struct Item {
    label: &'static str,
    href: String,
    icon: Icon,
    /// Global shortcut doing the same thing, shown right-aligned.
    hint: Option<&'static str>,
}

fn item(label: &'static str, href: impl Into<String>, icon: Icon) -> Item {
    Item {
        label,
        href: href.into(),
        icon,
        hint: None,
    }
}

impl Item {
    fn hint(mut self, hint: &'static str) -> Self {
        self.hint = Some(hint);
        self
    }
}

/// Menu items in groups (separated in the menu); shared by HTML and size.
fn groups(ctx: &MenuContext) -> Vec<Vec<Item>> {
    let mut groups: Vec<Vec<Item>> = Vec::new();
    let password_tools = || {
        vec![
            item("Generate password", "context://gen-password", Icon::Key),
            item("Fill / open vault", "context://fill-password", Icon::Lock).hint("Ctrl+Shift+P"),
        ]
    };

    if let Some(link) = ctx.link.as_deref().filter(|s| !s.is_empty()) {
        let enc = url_component(link);
        groups.push(vec![
            item(
                "Open link in new page",
                format!("context://open-link?url={enc}"),
                Icon::Add,
            ),
            item(
                "Open link in system browser",
                format!("context://external?url={enc}"),
                Icon::Open,
            ),
            item(
                "Copy link",
                format!("context://copy?text={enc}"),
                Icon::Copy,
            ),
        ]);
    }

    if let Some(img) = ctx.image.as_deref().filter(|s| !s.is_empty()) {
        let enc = url_component(img);
        groups.push(vec![
            item(
                "Copy image address",
                format!("context://copy?text={enc}"),
                Icon::Copy,
            ),
            item(
                "Save image",
                format!("context://save-image?url={enc}"),
                Icon::Download,
            ),
        ]);
    }

    if ctx.password_field {
        groups.push(password_tools());
    }

    if !ctx.on_canvas {
        let mut page = Vec::new();
        if let Some(url) = ctx.page_url.as_deref().filter(|s| !s.is_empty()) {
            let enc = url_component(url);
            if ctx.link.is_none() {
                page.push(
                    item(
                        "Copy page URL",
                        format!("context://copy?text={enc}"),
                        Icon::Copy,
                    )
                    .hint("Ctrl+Shift+C"),
                );
            }
            page.push(
                item(
                    "Open page in system browser",
                    format!("context://external?url={enc}"),
                    Icon::Open,
                )
                .hint("Ctrl+Shift+O"),
            );
        }
        page.push(item("Reader mode", "context://reader", Icon::Eye).hint("Ctrl+Shift+R"));
        groups.push(page);
        if !ctx.password_field {
            groups.push(password_tools());
        }
        groups.push(vec![item(
            "Close page",
            "context://close-page",
            Icon::Close,
        )
        .hint("Ctrl+W")]);
    }

    groups.push(vec![
        item("New page", "context://new-page", Icon::Add).hint("Ctrl+T"),
        item("New workspace", "context://save-workspace", Icon::Layers).hint("Ctrl+N"),
    ]);
    groups
}

// Menu geometry in CSS px — the CSS below and `menu_css_size` share these
// (a test checks they agree), so the tile fits the menu with no scrollbar.
const PAD: f32 = 5.0;
const ROW: f32 = 30.0;
const SEP: f32 = 9.0;
const BORDER: f32 = 1.0;
const WIDTH: f32 = 328.0;

const CSS: &str = r#"
:root { --pad: 5px; --row: 30px; --sep: 9px; }
html, body { height: 100%; overflow: hidden; }
body.compact {
  padding: var(--pad);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  font-size: 13px;
  user-select: none;
}
.menu { display: flex; flex-direction: column; }
.menu .row {
  height: var(--row);
  min-height: 0;
  gap: 10px;
  padding: 0 8px;
  border: 0;
  border-radius: var(--radius-sm);
  color: var(--fg);
  cursor: pointer;
}
.menu .row > svg { width: 16px; height: 16px; flex-shrink: 0; color: var(--muted); }
.menu .row .label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.menu .row .hint { flex-shrink: 0; color: var(--muted); font-family: var(--mono); font-size: 11px; }
.menu .row:hover { background: transparent; }
/* ui.js moves the selection with the mouse too: one highlight, never two. */
.menu .row.is-active { background: var(--accent); }
.menu .row.is-active, .menu .row.is-active > svg, .menu .row.is-active .hint { color: var(--accent-fg); }
.menu .row.is-selected::before { display: none; }
.menu .sep { height: 1px; margin: 4px 6px; background: var(--hairline); flex-shrink: 0; }
"#;

/// CSS-pixel size of the menu view (matches the CSS above; no scrollbars).
pub fn menu_css_size(ctx: &MenuContext) -> (f32, f32) {
    let groups = groups(ctx);
    let rows = groups.iter().map(Vec::len).sum::<usize>().max(1) as f32;
    let seps = groups.len().saturating_sub(1) as f32;
    (WIDTH, (PAD + BORDER) * 2.0 + rows * ROW + seps * SEP)
}

pub(crate) fn page_url(theme: &Theme, ctx: &MenuContext) -> String {
    let groups = groups(ctx);
    let body = html! {
        nav class="menu" role="menu" aria-label="Context menu" {
            @for (i, group) in groups.iter().enumerate() {
                @if i > 0 { div class="sep" role="separator" {} }
                @for entry in group {
                    a class="row" href=(entry.href) data-nav role="menuitem" {
                        (entry.icon)
                        span class="label" { (entry.label) }
                        @if let Some(hint) = entry.hint { span class="hint" { (hint) } }
                    }
                }
            }
        }
    };
    let html = ui::Doc::new(theme, "Menu")
        .class("compact")
        .escape("context://dismiss")
        .keys(&[("Enter", "Run selected item")])
        .css(CSS)
        .render(body);
    super::serve(super::kind::CONTEXT_MENU, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    #[test]
    fn size_constants_match_the_css() {
        for (var, value) in [("--pad", PAD), ("--row", ROW), ("--sep", SEP)] {
            assert!(CSS.contains(&format!("{var}: {value}px")), "{var}");
        }
        // .sep = 1px line + 4px margin above and below.
        assert_eq!(SEP, 1.0 + 4.0 * 2.0);
    }

    #[test]
    fn groups_follow_the_hit_target() {
        let canvas = MenuContext {
            on_canvas: true,
            ..Default::default()
        };
        let labels = |ctx: &MenuContext| -> Vec<&'static str> {
            groups(ctx).iter().flatten().map(|i| i.label).collect()
        };
        assert_eq!(labels(&canvas), ["New page", "New workspace"]);
        assert_eq!(menu_css_size(&canvas).1, (PAD + BORDER) * 2.0 + 2.0 * ROW);

        let field = MenuContext {
            page_url: Some("https://a.test/".into()),
            password_field: true,
            ..Default::default()
        };
        let all = labels(&field);
        // Password tools once, first, when the click was on a password field.
        assert_eq!(all.iter().filter(|l| **l == "Generate password").count(), 1);
        assert_eq!(all[0], "Generate password");
        assert!(all.contains(&"Copy page URL"));
    }

    #[test]
    fn menu_is_keyboard_driven_and_escape_dismisses() {
        let ctx = MenuContext {
            page_url: Some("https://a.test/".into()),
            ..Default::default()
        };
        let html = served_html(&page_url(&THEMES[0], &ctx));
        assert!(html.contains("class=\"compact\""));
        assert!(html.contains("data-escape=\"context://dismiss\""));
        assert!(html.contains("class=\"row\" href=\"context://reader\" data-nav"));
        assert!(html.contains("role=\"separator\""));
        assert!(html.contains("<span class=\"hint\">Ctrl+W</span>"));
    }
}
