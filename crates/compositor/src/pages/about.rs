// About page (palette: "About spatial-browser"): versions, where data
// lives, build options. Static; its links are ordinary built-in actions.

use super::ui::{self, html, Icon};
use crate::output::Theme;
use crate::persistence::settings;

/// `154.0.34+g14c5a08+chromium-154.0.8037.98` → (CEF, Chromium).
pub(crate) fn engine_versions() -> (String, String) {
    let raw = String::from_utf8_lossy(cef::sys::CEF_VERSION);
    let raw = raw.trim_end_matches('\0');
    let cef = raw.split('+').next().unwrap_or(raw).to_string();
    let chromium = raw
        .split('+')
        .find_map(|p| p.strip_prefix("chromium-"))
        .unwrap_or("?")
        .to_string();
    (cef, chromium)
}

pub(crate) fn page_url(theme: &Theme) -> String {
    let (cef, chromium) = engine_versions();
    let startup = settings::startup();
    let config = crate::persistence::config_path("");
    let rows: [(&str, String); 6] = [
        ("Version", env!("CARGO_PKG_VERSION").to_string()),
        ("CEF", cef),
        ("Chromium", chromium),
        (
            "GPU",
            match startup.gpu {
                settings::GpuPower::Low => "Integrated (low power)".into(),
                settings::GpuPower::High => "Discrete (high performance)".into(),
            },
        ),
        (
            "Page painting",
            if startup.shared_texture() {
                "GPU shared textures".into()
            } else {
                "CPU copy".into()
            },
        ),
        ("Data folder", config.display().to_string()),
    ];
    let body = html! {
        main class="page" {
            (ui::head("spatial-browser", Some(html! {
                "A spatial canvas web browser · CEF + winit + wgpu"
            }), html! {}))
            div class="list" {
                @for (k, v) in &rows {
                    div class="row card" data-nav data-text=(format!("{k} {v}")) {
                        div class="main" {
                            span class="sub" { (k) }
                            span class="title mono" { (v) }
                        }
                    }
                }
            }
            h2 { "More" }
            div class="cluster" {
                a class="btn sm" href="settings://open/help" { (Icon::Info) "Keyboard shortcuts" }
                a class="btn sm" href="settings://tab/performance" { (Icon::Gear) "Settings" }
                a class="btn sm" href="settings://open/scripts" { (Icon::Code) "Scripts & styles" }
            }
            p class="small muted" style="margin-top:16px" {
                "Source, wiki and releases: github.com/Loafer19/spatial-browser · "
                "Everything stays on this machine: history, bookmarks, the password vault and favicons are local files."
            }
        }
    };
    let html = ui::Doc::new(theme, "About spatial-browser")
        .no_row()
        .render(body);
    super::serve(super::kind::ABOUT, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    #[test]
    fn shows_versions() {
        let (cef, chromium) = engine_versions();
        assert!(cef.starts_with("154."), "{cef}");
        assert!(chromium.starts_with("154."), "{chromium}");
        let html = served_html(&page_url(&THEMES[0]));
        assert!(html.contains(env!("CARGO_PKG_VERSION")));
        assert!(html.contains(&chromium));
    }
}
