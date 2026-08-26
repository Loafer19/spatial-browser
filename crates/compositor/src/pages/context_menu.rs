// Right-click context menu — small ephemeral data: page at the cursor.
// Items depend on hit target (canvas / page / link / image / password).

use super::html_escape;
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

/// Build the label/href list (shared by HTML and size estimate).
fn items(ctx: &MenuContext) -> Vec<(&'static str, String)> {
    let mut items: Vec<(&'static str, String)> = Vec::new();

    if let Some(link) = ctx.link.as_deref().filter(|s| !s.is_empty()) {
        let enc = urlencoding_minimal(link);
        items.push((
            "Open link in new page",
            format!("context://open-link?url={enc}"),
        ));
        items.push(("Copy link", format!("context://copy?text={enc}")));
    }

    if let Some(img) = ctx.image.as_deref().filter(|s| !s.is_empty()) {
        let enc = urlencoding_minimal(img);
        items.push(("Copy image address", format!("context://copy?text={enc}")));
        items.push(("Save image", format!("context://save-image?url={enc}")));
    }

    if ctx.password_field {
        items.push(("Generate password", "context://gen-password".into()));
        items.push(("Fill / open vault", "context://fill-password".into()));
    }

    if !ctx.on_canvas {
        if let Some(url) = ctx.page_url.as_deref().filter(|s| !s.is_empty()) {
            if ctx.link.is_none() {
                let enc = urlencoding_minimal(url);
                items.push(("Copy page URL", format!("context://copy?text={enc}")));
            }
        }
        items.push(("Reader mode", "context://reader".into()));
        if !ctx.password_field {
            items.push(("Generate password", "context://gen-password".into()));
            items.push(("Fill / open vault", "context://fill-password".into()));
        }
        items.push(("Close page", "context://close-page".into()));
    }

    items.push(("New page", "context://new-page".into()));
    items.push(("Save as workspace", "context://save-workspace".into()));
    items
}

/// CSS-pixel size of the menu view (matches HTML layout; no scrollbars).
pub fn menu_css_size(ctx: &MenuContext) -> (f32, f32) {
    let n = items(ctx).len().max(1) as f32;
    // padding 6*2 + n * row (8+8 padding + ~17 line) — keep in sync with page_url CSS
    const PAD: f32 = 6.0;
    const ROW: f32 = 34.0;
    const WIDTH: f32 = 220.0;
    (WIDTH, PAD * 2.0 + n * ROW)
}

pub(crate) fn page_url(theme: &Theme, ctx: &MenuContext) -> String {
    let radius = theme.css_radius();
    let radius_inner = theme.css_radius_inner();
    let items = items(ctx);

    let mut rows = String::new();
    for (label, href) in &items {
        rows.push_str(&format!(
            "<a class=\"item\" href=\"{href}\" \
             style=\"display:block;box-sizing:border-box;width:100%;padding:8px 10px;\
             text-decoration:none;color:{fg};border-radius:{radius_inner};\
             overflow:hidden;text-overflow:ellipsis;white-space:nowrap\">{label}</a>",
            label = html_escape(label),
            fg = theme.help_fg,
        ));
    }

    // overflow:hidden — never show scrollbars inside the tiny CEF view.
    // width/height 100% fill the view sized by menu_css_size.
    format!(
        "data:text/html;charset=utf-8,\
         <html style=\"margin:0;width:100%;height:100%;overflow:hidden\">\
         <body style=\"margin:0;padding:6px;box-sizing:border-box;width:100%;height:100%;\
         overflow:hidden;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:13px;line-height:1.3;\
         border:1px solid {border};border-radius:{radius}\">\
         <style>\
         a.item:hover{{background:{card};color:{heading}}}\
         *{{box-sizing:border-box}}\
         </style>\
         <div style=\"width:100%;overflow:hidden\">{rows}</div></body></html>",
        bg = theme.help_bg,
        fg = theme.help_fg,
        border = theme.help_card_border,
        card = theme.help_card_bg,
        heading = theme.help_heading,
    )
}

fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
