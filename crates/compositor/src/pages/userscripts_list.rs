// Ctrl+Shift+U: userscripts + userstyles on the ui design system. Scripts
// and styles share this page; `userscripts://reload` reloads both from disk.

use super::ui::{self, html, Icon, Markup};
use crate::output::Theme;
use crate::userscripts::{RunAt, UserScript};
use crate::userstyles::UserStyle;

const CSS: &str = r#"
.row .switch { pointer-events: none; }
.row.off .title { opacity: .6; }
.row .matches { font-family: var(--mono); font-size: 11.5px; word-break: break-all; white-space: normal; }
"#;

fn item(
    toggle: &str,
    name: &str,
    file: &str,
    detail: &str,
    matches: &[String],
    on: bool,
) -> Markup {
    let matches = matches.join(", ");
    html! {
        div class={ "row card" @if !on { " off" } } data-open=(toggle)
            data-text=(format!("{name} {file} {matches}")) {
            div class="main" {
                span class="title" { (name) }
                span class="sub" { span class="mono" { (file) } @if !detail.is_empty() { " · " (detail) } }
                @if !matches.is_empty() { span class="sub matches muted" { (matches) } }
            }
            span class="switch" aria-hidden="true" title=(if on { "On" } else { "Off" }) {
                input type="checkbox" tabindex="-1" checked[on];
                span {}
            }
        }
    }
}

fn run_at(r: &RunAt) -> &'static str {
    match r {
        RunAt::DocumentStart => "document-start",
        RunAt::DocumentEnd => "document-end",
        RunAt::DocumentIdle => "document-idle",
    }
}

pub(crate) fn page_url(theme: &Theme, scripts: &[UserScript], styles: &[UserStyle]) -> String {
    let on =
        scripts.iter().filter(|s| s.enabled).count() + styles.iter().filter(|s| s.enabled).count();
    let body = html! {
        main class="page" {
            (ui::head("Scripts & styles", Some(html! {
                (on) " of " (scripts.len() + styles.len()) " on · Enter toggles · "
                code { "spatial-ui" } " matches built-in pages"
            }), html! {
                (ui::search("q", "", "Filter", Some(".page")))
                a class="btn sm primary" href="userscripts://reload" { (Icon::Refresh) "Reload" }
            }))
            div class="cluster" style="margin-bottom:8px" {
                a class="btn sm ghost" href="userscripts://open-dir" { (Icon::Folder) "Scripts folder" }
                a class="btn sm ghost" href="userscripts://open-styles-dir" { (Icon::Folder) "Styles folder" }
            }
            div data-group {
                h2 { "Userscripts" }
                @if scripts.is_empty() {
                    (ui::empty(Icon::Code, html! {
                        "No scripts yet. Drop a " code { ".js" } " with " code { "// @match" }
                        " into the scripts folder, then Reload."
                    }))
                }
                div class="list" {
                    @for s in scripts {
                        (item(&format!("userscripts://toggle/{}", super::url_component(&s.file_name)),
                            &s.name, &s.file_name, run_at(&s.run_at), &s.matches, s.enabled))
                    }
                }
            }
            div data-group {
                h2 { "Userstyles" }
                @if styles.is_empty() {
                    (ui::empty(Icon::Document, html! {
                        "No styles yet. Drop a " code { ".css" } " with " code { "/* @match */" }
                        " into the styles folder, then Reload."
                    }))
                }
                div class="list" {
                    @for s in styles {
                        (item(&format!("userscripts://toggle-style/{}", super::url_component(&s.file_name)),
                            &s.name, &s.file_name, "", &s.matches, s.enabled))
                    }
                }
            }
        }
    };
    let html = ui::Doc::new(theme, "Scripts & styles")
        .css(CSS)
        .render(body);
    super::serve(super::kind::USERSCRIPTS, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    #[test]
    fn rows_toggle_by_encoded_file_name_and_escape_metadata() {
        let scripts = [UserScript {
            file_name: "a b'<x>.js".into(),
            name: "<img src=x onerror=1>".into(),
            matches: vec!["https://*/*\"><svg>".into()],
            excludes: vec![],
            run_at: RunAt::DocumentIdle,
            code: String::new(),
            enabled: false,
        }];
        let html = served_html(&page_url(&THEMES[0], &scripts, &[]));
        assert!(html.contains("data-open=\"userscripts://toggle/a%20b%27%3Cx%3E.js\""));
        assert!(!html.contains("<img src=x") && !html.contains("<svg>\""));
        assert!(html.contains("No styles yet"));
        assert!(html.contains("userscripts://reload"));
    }
}
