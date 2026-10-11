// "Save password?" prompt: a small ephemeral tile above the page that
// offered a login, owned by the compositor. Unlike a banner in the page's
// own DOM, the page can't press Save itself or hide the prompt, and it
// survives the post-login navigation. The password never reaches this HTML.

use super::ui::{self, html};
use super::{kind, serve};
use crate::output::Theme;

const CSS: &str = r#"
html, body { height: 100%; overflow: hidden; }
body.compact {
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  gap: 10px;
  padding: 10px 12px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
}
.who { display: flex; align-items: center; gap: 10px; min-width: 0; }
.who .main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
.who .title { color: var(--heading); font-weight: 650; }
.who .sub { color: var(--muted); font-size: 12.5px; }
.actions { display: flex; align-items: center; gap: 6px; }
@media (max-width: 300px) { .long { display: none; } }
"#;

/// `is_update`: the login would overwrite a saved entry's password, which
/// the prompt says plainly so an unexpected offer stands out.
pub fn page_url(theme: &Theme, host: &str, username: &str, is_update: bool) -> String {
    let title = if is_update {
        "Update saved password?"
    } else {
        "Save password?"
    };
    let body = html! {
        div class="who" {
            (ui::favicon_lg(host, host))
            div class="main" {
                span class="title ellipsis" { (title) }
                span class="sub ellipsis" title=(host) {
                    @if !username.is_empty() { (username) " · " }
                    (host)
                }
            }
        }
        div class="actions" {
            a class="btn ghost sm" href="password://go/prompt-never" title="Never offer to save on this site" {
                // One span: .btn's flex gap would split "Never" from the rest.
                span { "Never" span class="long" { " for this site" } }
            }
            span class="spacer" {}
            a class="btn sm" href="password://go/prompt-dismiss" { "Not now" }
            a class="btn primary sm" href="password://go/prompt-save" autofocus { "Save" }
        }
    };
    let html = ui::Doc::new(theme, title)
        .class("compact")
        .escape("password://go/prompt-dismiss")
        .keys(&[("Enter", "Save"), ("Tab", "Next button")])
        .css(CSS)
        .no_row()
        .render(body);
    serve(kind::SAVE_PROMPT, html)
}

#[cfg(test)]
mod tests {
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    #[test]
    fn escapes_host_and_username() {
        let html = served_html(&super::page_url(
            &THEMES[0],
            "a.test",
            "<img src=x onerror=alert(1)>",
            false,
        ));
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;img"));
    }

    #[test]
    fn offers_save_never_and_not_now() {
        let html = served_html(&super::page_url(&THEMES[0], "a.test", "u", true));
        assert!(html.contains("Update saved password?"));
        for action in ["prompt-save", "prompt-never", "prompt-dismiss"] {
            assert!(
                html.contains(&format!("password://go/{action}")),
                "{action}"
            );
        }
        assert!(html.contains("data-escape=\"password://go/prompt-dismiss\""));
        let fresh = served_html(&super::page_url(&THEMES[0], "a.test", "", false));
        assert!(fresh.contains("Save password?") && !fresh.contains(" · "));
    }
}
