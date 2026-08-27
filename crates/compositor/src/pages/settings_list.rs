// Ctrl+, settings: tabbed General / Blocking / Appearance.
// Tab clicks use settings://tab/… so the active tab survives refresh
// after toggles. See PENDING_SETTINGS_ACTION.

use super::{html_escape, CHECKMARK_SVG_PATH, LIST_NAV_SCRIPT, TRASH_SVG_PATH};
use crate::output::{Theme, THEMES};
use crate::persistence::settings::AppSettings;
use crate::reader_mode::READER_THEMES;

const SEARCH_ENGINES: &[(&str, &str)] = &[
    ("Google", "https://www.google.com/search?q="),
    ("DuckDuckGo", "https://duckduckgo.com/?q="),
    ("Bing", "https://www.bing.com/search?q="),
];

const FRAME_RATES: &[u32] = &[60, 90, 120];

fn checkmark(theme: &Theme, current: bool) -> String {
    if !current {
        return String::new();
    }
    format!(
        "<svg width=\"16\" height=\"16\" viewBox=\"0 0 24 24\" fill=\"{accent}\">\
         <path d=\"{CHECKMARK_SVG_PATH}\"/></svg>",
        accent = theme.help_key_bg,
    )
}

fn on_off(v: bool) -> &'static str {
    if v {
        "On"
    } else {
        "Off"
    }
}

fn section_h2(theme: &Theme, title: &str) -> String {
    format!(
        "<h2 style=\"margin:16px 0 8px;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;color:{heading};opacity:0.8\">{title}</h2>",
        heading = theme.help_heading,
    )
}

fn toggle_row(theme: &Theme, href: &str, label: &str, state: bool) -> String {
    format!(
        "<div class=\"list-row\" data-open=\"{href}\" \
         onclick=\"location='{href}'\" \
         style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
         cursor:pointer;background:{card_bg};border-radius:8px;\
         border:1px solid {card_border}\">\
         <span style=\"flex:1;color:{fg}\">{label}</span>\
         <span style=\"flex-shrink:0;color:{fg};opacity:0.7\">{state}</span>\
         </div>",
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        fg = theme.help_fg,
        state = on_off(state),
    )
}

/// Subscription-style row: title, subtitle, On/Off.
fn list_row(
    theme: &Theme,
    href: &str,
    title: &str,
    subtitle: &str,
    enabled: bool,
    available: bool,
) -> String {
    let state = if !available {
        "Soon"
    } else {
        on_off(enabled)
    };
    let opacity = if available { "1" } else { "0.55" };
    let cursor = if available { "pointer" } else { "default" };
    let onclick = if available {
        format!("onclick=\"location='{href}'\"")
    } else {
        String::new()
    };
    format!(
        "<div class=\"list-row\" data-open=\"{href}\" {onclick} \
         style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
         cursor:{cursor};background:{card_bg};border-radius:8px;\
         border:1px solid {card_border};opacity:{opacity}\">\
         <div style=\"flex:1;min-width:0\">\
         <div style=\"color:{fg};font-weight:600\">{title}</div>\
         <div style=\"color:{fg};opacity:0.6;font-size:12px;margin-top:2px\">{subtitle}</div>\
         </div>\
         <span style=\"flex-shrink:0;color:{fg};opacity:0.7\">{state}</span>\
         </div>",
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        fg = theme.help_fg,
    )
}

fn choice_row(theme: &Theme, href: &str, label: &str, current: bool) -> String {
    format!(
        "<div class=\"list-row\" data-open=\"{href}\" \
         onclick=\"location='{href}'\" \
         style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
         cursor:pointer;background:{card_bg};border-radius:8px;\
         border:1px solid {card_border}\">\
         <span style=\"flex:1;color:{fg}\">{label}</span>{checkmark}</div>",
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        fg = theme.help_fg,
        checkmark = checkmark(theme, current),
    )
}

fn tab_bar(theme: &Theme, active: &str) -> String {
    let mut html = String::from(
        "<div style=\"display:flex;gap:6px;margin:0 0 20px;flex-wrap:wrap\">",
    );
    for (id, label) in [
        ("general", "General"),
        ("blocking", "Blocking"),
        ("appearance", "Appearance"),
    ] {
        let on = active == id;
        let (bg, fg, border) = if on {
            (theme.help_key_bg, theme.help_key_fg, theme.help_key_bg)
        } else {
            (theme.help_card_bg, theme.help_fg, theme.help_card_border)
        };
        html.push_str(&format!(
            "<a href=\"settings://tab/{id}\" \
             style=\"text-decoration:none;padding:8px 14px;border-radius:8px;\
             border:1px solid {border};background:{bg};color:{fg};font-size:13px;\
             font-weight:600\">{label}</a>",
        ));
    }
    html.push_str("</div>");
    html
}

fn general_panel(theme: &Theme, settings: &AppSettings) -> String {
    let mut rows = String::new();
    rows.push_str(&section_h2(theme, "Privacy"));
    rows.push_str(&toggle_row(
        theme,
        "settings://toggle-clean-urls",
        "Strip tracking parameters from links",
        settings.clean_urls_enabled,
    ));

    rows.push_str(&section_h2(theme, "Default search engine"));
    for (name, url) in SEARCH_ENGINES {
        rows.push_str(&choice_row(
            theme,
            &format!("settings://search-engine?engine={}", urlencoding_lite(url)),
            name,
            settings.default_search_engine == *url,
        ));
    }

    rows.push_str(&section_h2(theme, "Performance"));
    for fps in FRAME_RATES {
        rows.push_str(&choice_row(
            theme,
            &format!("settings://frame-rate/{fps}"),
            &format!("{fps} fps"),
            settings.target_fps == *fps,
        ));
    }
    rows
}

fn blocking_panel(theme: &Theme, settings: &AppSettings) -> String {
    let mut rows = String::new();
    rows.push_str(&toggle_row(
        theme,
        "settings://toggle-adblock",
        "Content filtering",
        settings.ad_block_enabled,
    ));
    rows.push_str(&format!(
        "<p style=\"margin:8px 0 0;color:{fg};opacity:0.65;font-size:12px\">\
         Master switch. When off, no filter list or layer runs.</p>",
        fg = theme.help_fg,
    ));

    rows.push_str(&section_h2(theme, "Filter lists"));
    rows.push_str(&list_row(
        theme,
        "settings://toggle-filter-list/peter_lowe",
        "Peter Lowe hosts",
        "Built-in ad/tracker domains (~3500) · request block",
        settings.filter_lists.peter_lowe,
        true,
    ));
    rows.push_str(&list_row(
        theme,
        "settings://toggle-filter-list/easylist",
        "EasyList",
        "Ads · EasyList syntax · coming with filter engine",
        settings.filter_lists.easylist,
        false,
    ));
    rows.push_str(&list_row(
        theme,
        "settings://toggle-filter-list/easyprivacy",
        "EasyPrivacy",
        "Trackers · EasyList syntax · coming with filter engine",
        settings.filter_lists.easyprivacy,
        false,
    ));

    rows.push_str(&section_h2(theme, "Custom hosts"));
    rows.push_str(&format!(
        "<form method=\"get\" action=\"settings://add-host\" \
         style=\"display:flex;gap:8px;margin-bottom:8px\">\
         <input name=\"host\" placeholder=\"example.com\" \
         style=\"flex:1;min-width:0;background:{card_bg};color:{fg};\
         border:1px solid {card_border};border-radius:6px;padding:8px 10px;\
         font:inherit;font-size:14px\">\
         <button type=\"submit\" style=\"background:{key_bg};color:{key_fg};border:none;\
         border-radius:6px;padding:8px 16px;font:inherit;font-size:14px;cursor:pointer\">\
         Add</button></form>",
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        fg = theme.help_fg,
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    ));
    if settings.custom_blocked_hosts.is_empty() {
        rows.push_str(&super::empty_state(
            theme,
            "No custom hosts yet — add domains the built-in list misses.",
        ));
    }
    for (index, host) in settings.custom_blocked_hosts.iter().enumerate() {
        rows.push_str(&format!(
            "<div class=\"list-row\" data-open=\"settings://remove-host/{index}\" \
             style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             background:{card_bg};border-radius:8px;border:1px solid {card_border}\">\
             <span style=\"flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
             color:{fg}\">{host_html}</span>{delete_button}</div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            fg = theme.help_fg,
            host_html = html_escape(host),
            delete_button = super::icon_link_button(
                &format!("settings://remove-host/{index}"),
                "Remove",
                TRASH_SVG_PATH,
                "",
                theme,
            ),
        ));
    }

    rows.push_str(&section_h2(theme, "Advanced"));
    rows.push_str(&toggle_row(
        theme,
        "settings://toggle-filter-network",
        "Apply network rules (cancel requests)",
        settings.filter_network_enabled,
    ));
    rows.push_str(&toggle_row(
        theme,
        "settings://toggle-filter-cosmetic",
        "Apply cosmetic hiding (CSS)",
        settings.filter_cosmetic_enabled,
    ));
    rows.push_str(&format!(
        "<p style=\"margin:4px 0 0;color:{fg};opacity:0.55;font-size:12px\">\
         Cosmetic inject arrives with the filter engine.</p>",
        fg = theme.help_fg,
    ));
    rows.push_str(&toggle_row(
        theme,
        "settings://toggle-filter-scriptlets",
        "Apply scriptlets (experimental)",
        settings.filter_scriptlets_enabled,
    ));
    rows.push_str(&format!(
        "<p style=\"margin:4px 0 0;color:{fg};opacity:0.55;font-size:12px\">\
         Off by default when scriptlets ship — can break sites.</p>",
        fg = theme.help_fg,
    ));

    rows
}

fn appearance_panel(theme: &Theme, settings: &AppSettings) -> String {
    let mut rows = String::new();
    rows.push_str(&section_h2(theme, "UI theme"));
    for (index, candidate) in THEMES.iter().enumerate() {
        rows.push_str(&choice_row(
            theme,
            &format!("settings://theme/{index}"),
            candidate.name,
            candidate.name == theme.name,
        ));
    }

    rows.push_str(&section_h2(theme, "Reading mode (Ctrl+Shift+R)"));
    for (index, reader) in READER_THEMES.iter().enumerate() {
        rows.push_str(&choice_row(
            theme,
            &format!("settings://reader-theme/{index}"),
            reader.name,
            settings.reader_theme == index,
        ));
    }
    rows
}

pub(crate) fn page_url(theme: &Theme, settings: &AppSettings) -> String {
    let tab = AppSettings::normalize_tab(&settings.settings_tab);
    let panel = match tab {
        "blocking" => blocking_panel(theme, settings),
        "appearance" => appearance_panel(theme, settings),
        _ => general_panel(theme, settings),
    };

    format!(
        "data:text/html;charset=utf-8,{nav_script}\
         <style>{icon_hover}\
         .list-row:hover,.list-row.list-active{{background:{key_bg}!important}}\
         .list-row:hover span,.list-row.list-active span,\
         .list-row:hover div,.list-row.list-active div{{color:{key_fg}!important}}</style>\
         {body_open}\
         <h1 style=\"margin:0 0 12px;color:{heading};font-size:20px\">Settings</h1>\
         {tabs}\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{panel}</div></body>",
        nav_script = LIST_NAV_SCRIPT,
        icon_hover = super::icon_button_hover_css(theme),
        body_open = super::body_open(theme, ""),
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        heading = theme.help_heading,
        tabs = tab_bar(theme, tab),
        panel = panel,
    )
}

fn urlencoding_lite(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ':' => "%3A".to_string(),
            '/' => "%2F".to_string(),
            '?' => "%3F".to_string(),
            '=' => "%3D".to_string(),
            '&' => "%26".to_string(),
            c => c.to_string(),
        })
        .collect()
}
