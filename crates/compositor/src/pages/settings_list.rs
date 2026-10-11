// Ctrl+, settings on the ui design system. Every section is rendered; the
// active tab (settings://tab/…, kept across refreshes) shows one of them,
// and typing in the search box shows matching rows from all of them.

use super::ui::{self, html, Icon, Markup};
use crate::output::{Theme, THEMES};
use crate::persistence::settings::{
    self, AppSettings, GpuPower, OsrMode, Startup, FRAME_RATES, SETTINGS_TABS,
};
use crate::reader_mode::READER_THEMES;

pub(crate) const SEARCH_ENGINES: &[(&str, &str)] = &[
    ("Google", "https://www.google.com/search?q="),
    ("DuckDuckGo", "https://duckduckgo.com/?q="),
    ("Bing", "https://www.bing.com/search?q="),
];

const CSS: &str = r#"
.section-block { display: none; }
body[data-view="general"] .section-block[data-section="general"],
body[data-view="blocking"] .section-block[data-section="blocking"],
body[data-view="appearance"] .section-block[data-section="appearance"],
body[data-view="performance"] .section-block[data-section="performance"],
body.searching .section-block { display: block; }
body.searching .tabs { opacity: .5; }
.section-block > h2 .tab-name { display: none; }
body.searching .section-block > h2 .tab-name { display: inline; }
.row .switch { pointer-events: none; }
.row .check { color: var(--accent); display: inline-flex; width: 18px; height: 18px; }
.row .check svg { width: 18px; height: 18px; }
.inline-form { display: flex; gap: 8px; margin: 6px 0 4px; }
.inline-form.row { padding: 0; background: none; border: 0; }
.section-block { margin-bottom: 18px; }
.inline-form input { flex: 1; min-width: 0; }
.note { margin: 4px 2px 10px; }
"#;

// Search across tabs: while the box has text, every section shows.
const SCRIPT: &str = r#"
(function () {
  var box = document.querySelector('input[data-filter]');
  if (!box) return;
  function sync() { document.body.classList.toggle('searching', !!box.value.trim()); }
  box.addEventListener('input', sync);
  sync();
})();
"#;

/// `settings://…` actions carry fixed ids except the search URL.
fn url_component(s: &str) -> String {
    super::url_component(s)
}

fn toggle(href: &str, title: &str, sub: &str, on: bool) -> Markup {
    html! {
        div class="row card" data-open=(href) data-text=(format!("{title} {sub}")) {
            div class="main" {
                span class="title" { (title) }
                @if !sub.is_empty() { span class="sub" { (sub) } }
            }
            span class="switch" aria-hidden="true" {
                input type="checkbox" tabindex="-1" checked[on];
                span {}
            }
        }
    }
}

fn choice(href: &str, title: &str, sub: &str, current: bool) -> Markup {
    html! {
        div class={ "row card" @if current { " current" } } data-open=(href)
            data-text=(format!("{title} {sub}")) aria-checked=(current) {
            div class="main" {
                span class="title" { (title) }
                @if !sub.is_empty() { span class="sub" { (sub) } }
            }
            @if current { span class="check" { (Icon::Check) } }
        }
    }
}

fn section(id: &str, label: &str, title: &str, body: Markup) -> Markup {
    html! {
        div class="section-block" data-section=(id) data-group {
            h2 { (title) span class="tab-name muted small" { " · " (label) } }
            div class="list" { (body) }
        }
    }
}

fn tab_label(id: &str) -> &'static str {
    SETTINGS_TABS
        .iter()
        .find(|(t, _)| *t == id)
        .map_or("", |(_, l)| l)
}

fn general(s: &AppSettings) -> Markup {
    let preset = SEARCH_ENGINES
        .iter()
        .any(|(_, url)| *url == s.default_search_engine);
    html! {
        (section("general", tab_label("general"), "Privacy", html! {
            (toggle("settings://toggle-clean-urls", "Strip tracking parameters from links",
                "utm_*, fbclid, gclid … removed before navigation", s.clean_urls_enabled))
        }))
        (section("general", tab_label("general"), "Default search engine", html! {
            @for (name, url) in SEARCH_ENGINES {
                (choice(&format!("settings://search-engine?engine={}", url_component(url)),
                    name, url, s.default_search_engine == *url))
            }
            @if !preset {
                (choice("settings://tab/general", "Custom", &s.default_search_engine, true))
            }
            form class="inline-form row" data-text="custom search engine url" method="get"
                action="settings://search-engine" {
                input class="input mono" type="url" name="engine"
                    placeholder="https://example.com/search?q=" maxlength=(settings::MAX_SEARCH_URL)
                    value=[(!preset).then_some(s.default_search_engine.as_str())]
                    autocomplete="off" spellcheck="false" aria-label="Custom search URL";
                button class="btn sm" type="submit" { "Use custom" }
            }
            p class="note small muted" { "The search text is appended to this URL. http(s) only." }
        }))
        (section("general", tab_label("general"), "More", html! {
            (choice("settings://open/scripts", "Scripts & styles", "Userscripts and CSS (Ctrl+Shift+U)", false))
            (choice("settings://open/help", "Keyboard shortcuts", "Every shortcut (F1)", false))
        }))
    }
}

fn blocking(s: &AppSettings) -> Markup {
    html! {
        (section("blocking", tab_label("blocking"), "Content filtering", html! {
            (toggle("settings://toggle-adblock", "Content filtering",
                "Master switch. When off, no filter list or layer runs.", s.ad_block_enabled))
        }))
        (section("blocking", tab_label("blocking"), "Filter lists", html! {
            (toggle("settings://toggle-filter-list/peter_lowe", "Peter Lowe hosts",
                "Built-in ad/tracker domains (~3500) · request block", s.filter_lists.peter_lowe))
            (toggle("settings://toggle-filter-list/easylist", "EasyList",
                "Ads · EasyList / uBlock syntax", s.filter_lists.easylist))
            (toggle("settings://toggle-filter-list/easyprivacy", "EasyPrivacy",
                "Trackers · EasyList / uBlock syntax", s.filter_lists.easyprivacy))
        }))
        (section("blocking", tab_label("blocking"), "Custom hosts", html! {
            form class="inline-form row" data-text="custom hosts add block domain" method="get"
                action="settings://add-host" {
                input class="input" name="host" placeholder="example.com" autocomplete="off"
                    spellcheck="false" aria-label="Host to block";
                button class="btn sm" type="submit" { (Icon::Add) "Add" }
            }
            @if s.custom_blocked_hosts.is_empty() {
                p class="note small muted" { "No custom hosts yet — add domains the built-in list misses." }
            }
            @for (index, host) in s.custom_blocked_hosts.iter().enumerate() {
                @let remove = format!("settings://remove-host/{index}");
                div class="row card" data-nav data-delete=(remove) data-text=(host) {
                    span class="main" { span class="title mono" { (host) } }
                    div class="actions" { (ui::icon_link(&remove, "Remove", Icon::Trash, true)) }
                }
            }
        }))
        (section("blocking", tab_label("blocking"), "Advanced", html! {
            (toggle("settings://toggle-filter-network", "Network rules",
                "Cancel requests matched by the enabled lists", s.filter_network_enabled))
            (toggle("settings://toggle-filter-cosmetic", "Cosmetic hiding",
                "Hide elements matched by enabled lists (CSS)", s.filter_cosmetic_enabled))
            (toggle("settings://toggle-filter-scriptlets", "Scriptlets (experimental)",
                "Inject ##+js from enabled lists. Can break sites.", s.filter_scriptlets_enabled))
        }))
    }
}

fn appearance(theme: &Theme, s: &AppSettings) -> Markup {
    html! {
        (section("appearance", tab_label("appearance"), "UI theme", html! {
            @for (index, candidate) in THEMES.iter().enumerate() {
                (choice(&format!("settings://theme/{index}"), candidate.name,
                    if candidate.is_dark { "Dark" } else { "Light" }, candidate.name == theme.name))
            }
        }))
        (section("appearance", tab_label("appearance"), "Reading mode (Ctrl+Shift+R)", html! {
            @for (index, reader) in READER_THEMES.iter().enumerate() {
                (choice(&format!("settings://reader-theme/{index}"), reader.name, "",
                    s.reader_theme == index))
            }
        }))
    }
}

fn gpu_label(g: GpuPower) -> (&'static str, &'static str) {
    match g {
        GpuPower::Low => (
            "Integrated (power saving)",
            "Same GPU as Chromium — shared-texture painting works",
        ),
        GpuPower::High => (
            "Discrete (high performance)",
            "Hybrid laptops: usually needs CPU painting",
        ),
    }
}

fn osr_label(o: OsrMode) -> (&'static str, &'static str) {
    match o {
        OsrMode::Auto => ("Auto", "GPU shared textures, CPU on the discrete GPU"),
        OsrMode::Gpu => ("GPU shared textures", "Fastest; breaks across two GPUs"),
        OsrMode::Cpu => ("CPU copy", "Always works; more CPU per frame"),
    }
}

fn performance(s: &AppSettings, startup: &Startup) -> Markup {
    let (next_gpu, next_osr) = startup.next(s);
    html! {
        (section("performance", tab_label("performance"), "Frame rate", html! {
            @for fps in FRAME_RATES {
                (choice(&format!("settings://frame-rate/{fps}"), &format!("{fps} fps"),
                    "Applies immediately", s.target_fps == *fps))
            }
        }))
        @if startup.restart_pending(s) {
            div class="section-block" data-section="performance" {
                (ui::banner(ui::Tone::Info, "Restart spatial-browser to apply the GPU / painting change."))
            }
        }
        (section("performance", tab_label("performance"), "GPU (applies after restart)", html! {
            @if startup.gpu_env.is_some() {
                p class="note small muted" { "Pinned by " code { (settings::GPU_ENV) } " — saved choice is used without it." }
            }
            @for g in GpuPower::ALL {
                @let (title, sub) = gpu_label(g);
                @let sub = if g == startup.gpu { format!("{sub} · running now") } else { sub.to_string() };
                (choice(&format!("settings://gpu/{}", g.id()), title, &sub, s.gpu == g))
            }
        }))
        (section("performance", tab_label("performance"), "Page painting (applies after restart)", html! {
            @if startup.osr_env.is_some() {
                p class="note small muted" { "Pinned by " code { (settings::OSR_ENV) } " — saved choice is used without it." }
            }
            @for o in OsrMode::ALL {
                @let (title, sub) = osr_label(o);
                @let sub = if o == startup.osr { format!("{sub} · running now") } else { sub.to_string() };
                (choice(&format!("settings://osr/{}", o.id()), title, &sub, s.osr == o))
            }
            p class="note small muted" {
                "Now: " (if startup.shared_texture() { "GPU shared textures" } else { "CPU copy" })
                @if (next_gpu, next_osr) != (startup.gpu, startup.osr) { " · after restart: "
                    (gpu_label(next_gpu).0) ", " (osr_label(next_osr).0) }
            }
        }))
    }
}

pub(crate) fn page_url(theme: &Theme, s: &AppSettings) -> String {
    page_url_with(theme, s, &settings::startup())
}

pub(crate) fn page_url_with(theme: &Theme, s: &AppSettings, startup: &Startup) -> String {
    let tab = AppSettings::normalize_tab(&s.settings_tab);
    let tabs: Vec<ui::Tab> = SETTINGS_TABS
        .iter()
        .map(|(id, label)| ui::Tab {
            label,
            href: match *id {
                "general" => "settings://tab/general",
                "blocking" => "settings://tab/blocking",
                "appearance" => "settings://tab/appearance",
                _ => "settings://tab/performance",
            },
            active: *id == tab,
            count: None,
        })
        .collect();
    let body = html! {
        main class="page" {
            (ui::head("Settings", None, ui::search("q", "", "Search all settings", Some(""))))
            (ui::tabs(&tabs))
            (general(s))
            (blocking(s))
            (appearance(theme, s))
            (performance(s, startup))
            div class="empty hidden" data-filter-empty { (Icon::Search) div { "No setting matches." } }
        }
    };
    let html = ui::Doc::new(theme, "Settings")
        .view(tab)
        .css(CSS)
        .script(SCRIPT)
        .render(body);
    super::serve(super::kind::SETTINGS, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::tests::served_html;

    fn startup(gpu: GpuPower, osr: OsrMode) -> Startup {
        Startup {
            gpu,
            osr,
            gpu_env: None,
            osr_env: None,
        }
    }

    #[test]
    fn every_section_is_rendered_and_tab_selects_view() {
        let s = AppSettings {
            settings_tab: "performance".into(),
            ..Default::default()
        };
        let html = served_html(&page_url_with(
            &THEMES[0],
            &s,
            &startup(GpuPower::Low, OsrMode::Auto),
        ));
        assert!(html.contains("data-view=\"performance\""));
        for id in ["general", "blocking", "appearance", "performance"] {
            assert!(html.contains(&format!("data-section=\"{id}\"")), "{id}");
        }
        assert!(html.contains("data-filter=\"\""), "search covers the page");
        assert!(html.contains("settings://gpu/high"));
        assert!(html.contains("settings://osr/cpu"));
        assert!(html.contains("settings://open/scripts"));
        assert!(!html.contains("Restart spatial-browser"));
    }

    #[test]
    fn restart_banner_when_saved_startup_options_differ() {
        let s = AppSettings {
            gpu: GpuPower::High,
            ..Default::default()
        };
        let html = served_html(&page_url_with(
            &THEMES[0],
            &s,
            &startup(GpuPower::Low, OsrMode::Auto),
        ));
        assert!(html.contains("Restart spatial-browser to apply"));
        assert!(html.contains("after restart"));
    }

    #[test]
    fn custom_values_are_escaped() {
        let s = AppSettings {
            default_search_engine: "https://x.test/?q=\"><script>alert(1)</script>".into(),
            custom_blocked_hosts: vec!["<img src=x onerror=1>".into()],
            ..Default::default()
        };
        let html = served_html(&page_url_with(
            &THEMES[0],
            &s,
            &startup(GpuPower::Low, OsrMode::Auto),
        ));
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img src=x"));
        assert!(html.contains("settings://remove-host/0"));
    }
}
