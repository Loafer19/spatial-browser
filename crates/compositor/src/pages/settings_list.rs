// The Ctrl+, settings page: ad-block on/off, the omnibox's default
// search engine, a direct theme picker (Ctrl+Shift+Space already cycles
// themes — this is the same choice, just click instead of cycle), the
// target frame rate (60/90/120 — a monitor's own max refresh rate is
// still the real ceiling, this just gives the pipeline room to reach
// it), and the user's own additions to the ad/tracker blocklist (on top
// of cef-bridge's compiled-in one — see blocklist.rs's header comment
// for why that one stays static). See cef-bridge's OsrRequestHandler /
// app.rs's PENDING_SETTINGS_ACTION for how clicks on this page's
// `settings://...` links get handled.

use super::{html_escape, CHECKMARK_SVG_PATH, LIST_NAV_SCRIPT, TRASH_SVG_PATH};
use crate::output::{Theme, THEMES};
use crate::persistence::settings::AppSettings;

/// The default-search-engine choices offered — a fixed, known-good set
/// rather than a free-text URL template: no chance of saving a broken
/// one, at the cost of not supporting an arbitrary engine.
const SEARCH_ENGINES: &[(&str, &str)] = &[
    ("Google", "https://www.google.com/search?q="),
    ("DuckDuckGo", "https://duckduckgo.com/?q="),
    ("Bing", "https://www.bing.com/search?q="),
];

/// Frame-rate choices offered — CEF's own `windowless_frame_rate` and
/// the main event loop's pacing both get set to this (browser.rs's
/// `set_target_frame_rate`, main.rs's own loop reading `App::target_fps`
/// live). A monitor's actual max refresh rate is still the real
/// ceiling regardless of this setting; 120 just gives the pipeline room
/// to hit it if the display can show it.
const FRAME_RATES: &[u32] = &[60, 90, 120];

/// A small static checkmark (not a button) marking whichever choice in
/// a settings section is currently active — reuses `help_key_bg` as the
/// accent color, the same one `group-toggle button.active` already uses
/// in history_list.rs for the same "this one's selected" meaning.
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

/// Builds the settings page's `data:` URL. `pub(crate)` so app.rs can
/// rebuild this page in place after any change, same close+respawn
/// pattern as every other list page (see refresh_bookmarks_page).
pub(crate) fn page_url(theme: &Theme, settings: &AppSettings) -> String {
    let adblock_state = if settings.ad_block_enabled {
        "On"
    } else {
        "Off"
    };
    let mut rows = format!(
        "<h2 style=\"margin:0 0 8px;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;color:{heading};opacity:0.8\">Ad &amp; tracker blocking</h2>\
         <div class=\"list-row\" data-open=\"settings://toggle-adblock\" \
         onclick=\"location='settings://toggle-adblock'\" \
         style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
         cursor:pointer;background:{card_bg};border-radius:8px;\
         border:1px solid {card_border}\">\
         <span style=\"flex:1;color:{fg}\">Block known ad/tracker requests</span>\
         <span style=\"flex-shrink:0;color:{fg};opacity:0.7\">{adblock_state}</span>\
         </div>",
        heading = theme.help_heading,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        fg = theme.help_fg,
    );

    rows.push_str(&format!(
        "<h2 style=\"margin:16px 0 8px;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;color:{heading};opacity:0.8\">Default search engine</h2>",
        heading = theme.help_heading,
    ));
    for (name, url) in SEARCH_ENGINES {
        let is_current = settings.default_search_engine == *url;
        rows.push_str(&format!(
            "<div class=\"list-row\" data-open=\"settings://search-engine?engine={url_enc}\" \
             onclick=\"location='settings://search-engine?engine={url_enc}'\" \
             style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             cursor:pointer;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             <span style=\"flex:1;color:{fg}\">{name}</span>{checkmark}</div>",
            url_enc = urlencoding_lite(url),
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            fg = theme.help_fg,
            checkmark = checkmark(theme, is_current),
        ));
    }

    rows.push_str(&format!(
        "<h2 style=\"margin:16px 0 8px;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;color:{heading};opacity:0.8\">Theme</h2>",
        heading = theme.help_heading,
    ));
    for (index, candidate) in THEMES.iter().enumerate() {
        let is_current = candidate.name == theme.name;
        rows.push_str(&format!(
            "<div class=\"list-row\" data-open=\"settings://theme/{index}\" \
             onclick=\"location='settings://theme/{index}'\" \
             style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             cursor:pointer;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             <span style=\"flex:1;color:{fg}\">{name}</span>{checkmark}</div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            fg = theme.help_fg,
            name = candidate.name,
            checkmark = checkmark(theme, is_current),
        ));
    }

    rows.push_str(&format!(
        "<h2 style=\"margin:16px 0 8px;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;color:{heading};opacity:0.8\">Performance</h2>",
        heading = theme.help_heading,
    ));
    for fps in FRAME_RATES {
        let is_current = settings.target_fps == *fps;
        rows.push_str(&format!(
            "<div class=\"list-row\" data-open=\"settings://frame-rate/{fps}\" \
             onclick=\"location='settings://frame-rate/{fps}'\" \
             style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             cursor:pointer;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             <span style=\"flex:1;color:{fg}\">{fps} fps</span>{checkmark}</div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            fg = theme.help_fg,
            checkmark = checkmark(theme, is_current),
        ));
    }

    rows.push_str(&format!(
        "<h2 style=\"margin:16px 0 8px;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;color:{heading};opacity:0.8\">Your blocked hosts</h2>\
         <form method=\"get\" action=\"settings://add-host\" \
         style=\"display:flex;gap:8px;margin-bottom:8px\">\
         <input name=\"host\" placeholder=\"example.com\" \
         style=\"flex:1;min-width:0;background:{card_bg};color:{fg};\
         border:1px solid {card_border};border-radius:6px;padding:8px 10px;\
         font:inherit;font-size:14px\">\
         <button type=\"submit\" style=\"background:{key_bg};color:{key_fg};border:none;\
         border-radius:6px;padding:8px 16px;font:inherit;font-size:14px;cursor:pointer\">\
         Add</button></form>",
        heading = theme.help_heading,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        fg = theme.help_fg,
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    ));
    if settings.custom_blocked_hosts.is_empty() {
        rows.push_str(&super::empty_state(
            theme,
            "No hosts added yet &mdash; the list above blocks the common ones already.",
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

    format!(
        "data:text/html;charset=utf-8,{nav_script}\
         <style>{icon_hover}\
         .list-row:hover,.list-row.list-active{{background:{key_bg}!important}}\
         .list-row:hover span,.list-row.list-active span{{color:{key_fg}!important}}</style>\
         {body_open}\
         <h1 style=\"margin:0 0 20px;color:{heading};font-size:20px\">Settings</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        nav_script = LIST_NAV_SCRIPT,
        icon_hover = super::icon_button_hover_css(theme),
        body_open = super::body_open(theme, ""),
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        heading = theme.help_heading,
    )
}

/// Percent-encodes just enough of a URL template to survive as one
/// query-string value (`:`, `/`, `?`, `=`, `&`) — the templates in
/// `SEARCH_ENGINES` above are the only input, so this doesn't need to
/// handle arbitrary text.
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
