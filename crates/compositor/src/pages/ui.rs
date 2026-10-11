// Design system for the built-in pages: one stylesheet (ui.css) themed by
// CSS custom properties, one keyboard model (ui.js), and maud components.
// maud escapes every `(value)` it splices — text and attributes alike — so
// page code never calls `html_escape` by hand. `PreEscaped` is only for the
// app's own constants (CSS, JS, SVG paths).

// Components are adopted page by page; drop this once every page is on ui.
#![allow(dead_code)]

pub(crate) use maud::{html, Markup, PreEscaped};
use maud::{Render, DOCTYPE};

use crate::output::Theme;

const UI_CSS: &str = include_str!("ui.css");
const UI_JS: &str = include_str!("ui.js");

/// Keys every page understands (ui.js); shown first in the `?` overlay.
const COMMON_KEYS: &[(&str, &str)] = &[
    ("↑ ↓  j k", "Move selection"),
    ("Enter", "Open"),
    ("Del", "Delete selected"),
    ("/", "Search"),
    ("Esc", "Clear / close"),
    ("?", "This help"),
];

/// CSS custom properties for `theme` (ui.css derives the rest).
pub(crate) fn theme_vars(theme: &Theme) -> Vec<(&'static str, String)> {
    let radius = theme.help_radius.max(0.0);
    vec![
        ("--bg", theme.help_bg.to_string()),
        ("--fg", theme.help_fg.to_string()),
        ("--heading", theme.help_heading.to_string()),
        ("--surface", theme.help_card_bg.to_string()),
        ("--border", theme.help_card_border.to_string()),
        ("--accent", theme.help_key_bg.to_string()),
        ("--accent-fg", theme.help_key_fg.to_string()),
        ("--danger", theme.help_danger.to_string()),
        ("--success", theme.help_success.to_string()),
        ("--radius", format!("{radius}px")),
        ("--radius-sm", format!("{}px", (radius * 0.75).round())),
        ("--font", theme.help_font.to_string()),
        ("--mono", theme.help_mono.to_string()),
    ]
}

fn color_scheme(theme: &Theme) -> &'static str {
    if theme.is_dark {
        "dark"
    } else {
        "light"
    }
}

fn root_css(theme: &Theme) -> String {
    let mut css = format!(":root{{color-scheme:{};", color_scheme(theme));
    for (name, value) in theme_vars(theme) {
        css.push_str(name);
        css.push(':');
        css.push_str(&value);
        css.push(';');
    }
    css.push('}');
    css
}

/// JS that re-themes an already loaded page in place (`ui.theme`).
pub(crate) fn theme_js(theme: &Theme) -> String {
    let mut vars: serde_json::Map<String, serde_json::Value> = theme_vars(theme)
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.into()))
        .collect();
    vars.insert("color-scheme".into(), color_scheme(theme).into());
    format!("window.ui&&ui.theme({});", serde_json::Value::Object(vars))
}

/// JS that hands `value` to the page's `ui.on(name, …)` handler.
pub(crate) fn push_js(name: &str, value: &serde_json::Value) -> String {
    format!(
        "window.ui&&ui.push({},{value});",
        super::js_string_literal(name)
    )
}

/// Run `js` in a built-in page's main frame (no-op once it is gone).
pub(crate) fn run_js(browser: &cef::Browser, js: &str) {
    use cef::ImplBrowser;
    use cef::ImplFrame;
    if let Some(frame) = browser.main_frame() {
        frame.execute_java_script(Some(&js.into()), Some(&"".into()), 0);
    }
}

/// A built-in page's document. Build with `Doc::new`, adjust, `render`.
pub(crate) struct Doc<'a> {
    theme: &'a Theme,
    title: &'a str,
    class: &'a str,
    view: Option<&'a str>,
    escape: Option<&'a str>,
    keys: &'a [(&'a str, &'a str)],
    css: &'a str,
    script: &'a str,
    no_row: bool,
}

impl<'a> Doc<'a> {
    pub(crate) fn new(theme: &'a Theme, title: &'a str) -> Self {
        Self {
            theme,
            title,
            class: "",
            view: None,
            escape: None,
            keys: &[],
            css: "",
            script: "",
            no_row: false,
        }
    }

    /// `body` classes, e.g. `compact` for chrome-less tiles (find bar).
    pub(crate) fn class(mut self, class: &'a str) -> Self {
        self.class = class;
        self
    }

    /// `body[data-view]`: the current tab, so scroll memory is per tab.
    pub(crate) fn view(mut self, view: &'a str) -> Self {
        self.view = Some(view);
        self
    }

    /// Where Escape navigates when nothing on the page wants it.
    pub(crate) fn escape(mut self, url: &'a str) -> Self {
        self.escape = Some(url);
        self
    }

    /// Page-specific keys for the `?` overlay.
    pub(crate) fn keys(mut self, keys: &'a [(&'a str, &'a str)]) -> Self {
        self.keys = keys;
        self
    }

    /// Page-specific CSS (an app constant, not data).
    pub(crate) fn css(mut self, css: &'static str) -> Self {
        self.css = css;
        self
    }

    /// Page-specific script, run after the body (an app constant, not data;
    /// pass data through `data-*` attributes or `json_script`).
    pub(crate) fn script(mut self, script: &'static str) -> Self {
        self.script = script;
        self
    }

    /// Don't preselect the first row on load.
    pub(crate) fn no_row(mut self) -> Self {
        self.no_row = true;
        self
    }

    pub(crate) fn render(self, body: Markup) -> String {
        html! {
            (DOCTYPE)
            html lang="en" {
                head {
                    meta charset="utf-8";
                    meta name="color-scheme" content=(color_scheme(self.theme));
                    title { (self.title) }
                    style { (PreEscaped(root_css(self.theme))) (PreEscaped(UI_CSS)) (PreEscaped(self.css)) }
                    script { (PreEscaped(UI_JS)) }
                }
                body class=(self.class)
                    data-view=[self.view]
                    data-escape=[self.escape]
                    data-no-row[self.no_row] {
                    (body)
                    template id="ui-keys" {
                        div class="card" {
                            h3 { "Keyboard" }
                            div class="keys" {
                                @for (key, desc) in COMMON_KEYS.iter().chain(self.keys) {
                                    span { kbd { (key) } }
                                    span class="muted" { (desc) }
                                }
                            }
                        }
                    }
                    @if !self.script.is_empty() {
                        script { (PreEscaped(self.script)) }
                    }
                }
            }
        }
        .into_string()
    }
}

// ---- components --------------------------------------------------------------

/// Material-style 24×24 icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Add,
    Check,
    Clock,
    Close,
    Code,
    Copy,
    Download,
    Drag,
    Edit,
    Eye,
    Folder,
    Gear,
    Globe,
    Grid,
    History,
    Info,
    Key,
    Layers,
    Lock,
    Open,
    Pause,
    Play,
    Refresh,
    Search,
    Shield,
    Star,
    Trash,
    Zap,
}

impl Icon {
    fn path(self) -> &'static str {
        match self {
            Icon::Add => "M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z",
            Icon::Check => super::CHECKMARK_SVG_PATH,
            Icon::Clock => "M11.99 2C6.47 2 2 6.48 2 12s4.47 10 9.99 10C17.52 22 22 17.52 22 12S17.52 2 11.99 2zM12 20c-4.42 0-8-3.58-8-8s3.58-8 8-8 8 3.58 8 8-3.58 8-8 8zm.5-13H11v6l5.25 3.15.75-1.23-4.5-2.67z",
            Icon::Close => "M19 6.41 17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z",
            Icon::Code => "M9.4 16.6 4.8 12l4.6-4.6L8 6l-6 6 6 6 1.4-1.4zm5.2 0 4.6-4.6-4.6-4.6L16 6l6 6-6 6-1.4-1.4z",
            Icon::Copy => "M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z",
            Icon::Download => "M19 9h-4V3H9v6H5l7 7 7-7zM5 18v2h14v-2H5z",
            Icon::Drag => "M11 18c0 1.1-.9 2-2 2s-2-.9-2-2 .9-2 2-2 2 .9 2 2zm-2-8c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm0-6c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm6 4c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm0 2c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm0 6c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z",
            Icon::Edit => "M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04a.996.996 0 0 0 0-1.41l-2.34-2.34a.996.996 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z",
            Icon::Eye => "M12 4.5C7 4.5 2.73 7.61 1 12c1.73 4.39 6 7.5 11 7.5s9.27-3.11 11-7.5c-1.73-4.39-6-7.5-11-7.5zM12 17c-2.76 0-5-2.24-5-5s2.24-5 5-5 5 2.24 5 5-2.24 5-5 5zm0-8c-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3-1.34-3-3-3z",
            Icon::Folder => "M10 4H4c-1.1 0-1.99.9-1.99 2L2 18c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2h-8l-2-2z",
            Icon::Gear => "M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.488.488 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.484.484 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z",
            Icon::Globe => "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 17.93c-3.95-.49-7-3.85-7-7.93 0-.62.08-1.21.21-1.79L9 15v1c0 1.1.9 2 2 2v1.93zm6.9-2.54c-.26-.81-1-1.39-1.9-1.39h-1v-3c0-.55-.45-1-1-1H8v-2h2c.55 0 1-.45 1-1V7h2c1.1 0 2-.9 2-2v-.41c2.93 1.19 5 4.06 5 7.41 0 2.08-.8 3.97-2.1 5.39z",
            Icon::Grid => "M4 8h4V4H4v4zm6 12h4v-4h-4v4zm-6 0h4v-4H4v4zm0-6h4v-4H4v4zm6 0h4v-4h-4v4zm6-10v4h4V4h-4zm-6 4h4V4h-4v4zm6 6h4v-4h-4v4zm0 6h4v-4h-4v4z",
            Icon::History => "M13 3a9 9 0 0 0-9 9H1l3.89 3.89.07.14L9 12H6c0-3.87 3.13-7 7-7s7 3.13 7 7-3.13 7-7 7c-1.93 0-3.68-.79-4.94-2.06l-1.42 1.42A8.954 8.954 0 0 0 13 21a9 9 0 0 0 0-18zm-1 5v5l4.28 2.54.72-1.21-3.5-2.08V8H12z",
            Icon::Info => "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-6h2v6zm0-8h-2V7h2v2z",
            Icon::Key => "M12.65 10A5.99 5.99 0 0 0 7 6c-3.31 0-6 2.69-6 6s2.69 6 6 6a5.99 5.99 0 0 0 5.65-4H17v4h4v-4h2v-4H12.65zM7 14c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2z",
            Icon::Layers => "M11.99 18.54 4.62 12.81 3 14.07l9 7 9-7-1.63-1.27-7.38 5.74zM12 16l7.36-5.73L21 9l-9-7-9 7 1.63 1.27L12 16z",
            Icon::Lock => "M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2zm3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1 1.71 0 3.1 1.39 3.1 3.1v2z",
            Icon::Open => "M19 19H5V5h7V3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14c1.1 0 2-.9 2-2v-7h-2v7zM14 3v2h3.59l-9.83 9.83 1.41 1.41L19 6.41V10h2V3h-7z",
            Icon::Pause => "M6 19h4V5H6v14zm8-14v14h4V5h-4z",
            Icon::Play => "M8 5v14l11-7z",
            Icon::Refresh => "M17.65 6.35A7.958 7.958 0 0 0 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08A5.99 5.99 0 0 1 12 18c-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z",
            Icon::Search => "M15.5 14h-.79l-.28-.27A6.471 6.471 0 0 0 16 9.5 6.5 6.5 0 1 0 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z",
            Icon::Shield => "M12 1 3 5v6c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V5l-9-4z",
            Icon::Star => "M12 17.27 18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z",
            Icon::Trash => super::TRASH_SVG_PATH,
            Icon::Zap => "M7 2v11h3v9l7-12h-4l4-8z",
        }
    }
}

impl Render for Icon {
    fn render(&self) -> Markup {
        html! {
            svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {
                path d=(self.path()) {}
            }
        }
    }
}

/// Page header: title, optional subtitle line, actions on the right.
pub(crate) fn head(title: &str, sub: Option<Markup>, actions: Markup) -> Markup {
    html! {
        header class="page-head" {
            h1 { (title) }
            (actions)
            @if let Some(sub) = sub {
                div class="sub small muted" { (sub) }
            }
        }
    }
}

/// Icon-only `<a>` (row actions). `danger` tints it on hover.
pub(crate) fn icon_link(href: &str, title: &str, icon: Icon, danger: bool) -> Markup {
    html! {
        a class={ "icon-btn" @if danger { " danger" } } href=(href) title=(title) aria-label=(title) {
            (icon)
        }
    }
}

/// Icon-only submit button inside a `<form>`.
pub(crate) fn icon_submit(title: &str, icon: Icon) -> Markup {
    html! {
        button type="submit" class="icon-btn" title=(title) aria-label=(title) { (icon) }
    }
}

/// Site icon: the locally cached favicon over a letter fallback. Never
/// fetches from `host` (see persistence::favicons).
pub(crate) fn favicon(host: &str, label: &str) -> Markup {
    favicon_sized(host, label, false)
}

pub(crate) fn favicon_lg(host: &str, label: &str) -> Markup {
    favicon_sized(host, label, true)
}

fn favicon_sized(host: &str, label: &str, large: bool) -> Markup {
    let letter: String = label
        .chars()
        .find(|c| c.is_alphanumeric())
        .unwrap_or('?')
        .to_uppercase()
        .collect();
    let icon = crate::persistence::favicons::data_uri(host);
    html! {
        span class={ "favicon" @if large { " lg" } } aria-hidden="true" {
            (letter)
            @if let Some(uri) = icon { img src=(uri) alt=""; }
        }
    }
}

/// Search box. `filter`: `Some(selector)` filters `.row`s under it client-
/// side (`Some("")` = whole page); `None` for a server-side search form.
pub(crate) fn search(name: &str, value: &str, placeholder: &str, filter: Option<&str>) -> Markup {
    html! {
        label class="search" {
            (Icon::Search)
            input type="search" name=(name) value=(value) placeholder=(placeholder)
                autocomplete="off" spellcheck="false" data-filter=[filter];
            kbd { "/" }
        }
    }
}

pub(crate) struct Tab<'a> {
    pub label: &'a str,
    pub href: &'a str,
    pub active: bool,
    pub count: Option<usize>,
}

pub(crate) fn tabs(items: &[Tab]) -> Markup {
    html! {
        nav class="tabs" {
            @for tab in items {
                a class="tab" href=(tab.href) aria-current=[tab.active.then_some("page")] {
                    (tab.label)
                    @if let Some(count) = tab.count { span class="count" { (count) } }
                }
            }
        }
    }
}

/// Empty-list placeholder.
pub(crate) fn empty(icon: Icon, text: Markup) -> Markup {
    html! {
        div class="empty" { (icon) div { (text) } }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Tone {
    Info,
    Success,
    Danger,
}

/// One-line status message above a list.
pub(crate) fn banner(tone: Tone, text: &str) -> Markup {
    let (class, icon) = match tone {
        Tone::Info => ("banner", Icon::Info),
        Tone::Success => ("banner success", Icon::Check),
        Tone::Danger => ("banner danger", Icon::Info),
    };
    html! {
        div class=(class) role="status" { (icon) span { (text) } }
    }
}

/// `<script type="application/json" id=…>`: data for a page script, read
/// with `JSON.parse(document.getElementById(id).textContent)`. `<` is
/// escaped so the data can never close the element.
pub(crate) fn json_script(id: &str, value: &serde_json::Value) -> Markup {
    let json = value
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    html! {
        script type="application/json" id=(id) { (PreEscaped(json)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;

    #[test]
    fn document_inlines_theme_css_and_keyboard_script() {
        let theme = THEMES[0];
        let html = Doc::new(&theme, "T <x>")
            .view("saved")
            .escape("switcher://close")
            .render(html! { p { "hi" } });
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>T &lt;x&gt;</title>"));
        assert!(html.contains(&format!("--bg:{};", theme.help_bg)));
        assert!(html.contains("color-scheme:dark"));
        assert!(html.contains("window.ui = {"));
        assert!(html.contains("data-view=\"saved\""));
        assert!(html.contains("data-escape=\"switcher://close\""));
        assert!(html.contains("id=\"ui-keys\""));
    }

    #[test]
    fn components_escape_data() {
        let evil = "\"><img src=x onerror=alert(1)>";
        for markup in [
            head(evil, Some(html! { (evil) }), html! {}),
            icon_link(evil, evil, Icon::Trash, true),
            favicon(evil, evil),
            search(evil, evil, evil, Some(evil)),
            tabs(&[Tab {
                label: evil,
                href: evil,
                active: true,
                count: Some(3),
            }]),
            banner(Tone::Danger, evil),
        ] {
            let s = markup.into_string();
            assert!(!s.contains("<img src=x"), "{s}");
        }
    }

    #[test]
    fn json_script_cannot_close_its_element() {
        let s = json_script("d", &serde_json::json!({ "t": "</script><b>&" })).into_string();
        assert!(!s.contains("</script><b>"));
        assert!(s.contains("\\u003c/script\\u003e"));
    }

    #[test]
    fn live_theme_and_push_js_are_well_formed() {
        let js = theme_js(&THEMES[1]);
        assert!(js.starts_with("window.ui&&ui.theme({"));
        assert!(js.contains("\"--accent\""));
        let push = push_js("down'loads", &serde_json::json!([1]));
        assert_eq!(push, "window.ui&&ui.push('down\\u0027loads',[1]);");
    }

    #[test]
    fn every_theme_sets_every_variable() {
        for theme in THEMES {
            let vars = theme_vars(theme);
            assert_eq!(vars.len(), 13);
            assert!(vars.iter().all(|(_, v)| !v.is_empty()), "{}", theme.name);
        }
    }
}
