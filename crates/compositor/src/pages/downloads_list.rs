// The Ctrl+J downloads-list page: most-recent-first, each row a
// favicon+filename+source-host, click to open the file with the
// desktop's default handler, an "x" to forget the entry (the file
// itself stays on disk — this only edits the list). See cef-bridge's
// OsrRequestHandler / app.rs's PENDING_DOWNLOAD_ACTION for how a row
// click here gets handled, and cef-bridge's OsrDownloadHandler for how
// a download lands under ~/Downloads in the first place.

use super::{html_escape, LIST_NAV_SCRIPT, TRASH_SVG_PATH};
use crate::output::Theme;
use crate::persistence::bookmarks::host_of;
use crate::persistence::downloads::DownloadRecord;

/// Builds the downloads-list page's `data:` URL. `pub(crate)` so app.rs
/// can rebuild this page in place after a remove (same close+respawn
/// pattern as the bookmarks list — see refresh_bookmarks_page).
pub(crate) fn page_url(theme: &Theme, downloads: &[DownloadRecord]) -> String {
    let mut rows = String::new();
    if downloads.is_empty() {
        rows.push_str(&super::empty_state(theme, "No downloads yet."));
    }

    for (index, download) in downloads.iter().enumerate() {
        let filename = std::path::Path::new(&download.path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&download.path);
        let host = host_of(&download.url);
        let letter = filename
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        rows.push_str(&format!(
            "<div class=\"dl-row list-row\" data-open=\"download://open/{index}\" \
             onclick=\"location='download://open/{index}'\" \
             style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             cursor:pointer;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             {favicon}\
             <div style=\"flex:1;min-width:0;display:flex;flex-direction:column\">\
             <span style=\"overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
             color:{fg}\">{filename_html}</span>\
             <span style=\"overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
             color:{fg};opacity:0.6;font-size:12px\">{host_html}</span></div>\
             {remove_button}</div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            favicon = super::favicon_tile(host, &letter, theme),
            fg = theme.help_fg,
            filename_html = html_escape(filename),
            host_html = html_escape(host),
            remove_button = super::icon_link_button(
                &format!("download://remove/{index}"),
                "Remove from list",
                TRASH_SVG_PATH,
                "onclick=\"event.stopPropagation()\"",
                theme,
            ),
        ));
    }

    format!(
        "data:text/html,{nav_script}\
         <style>.dl-row:hover,.dl-row.list-active{{background:{key_bg}!important}}\
         .dl-row:hover span,.dl-row.list-active span{{color:{key_fg}!important}}\
         {icon_hover}</style>\
         {body_open}\
         <h1 style=\"margin:0 0 20px;color:{heading};font-size:20px\">Downloads</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        nav_script = LIST_NAV_SCRIPT,
        icon_hover = super::icon_button_hover_css(theme),
        body_open = super::body_open(theme, ""),
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        heading = theme.help_heading,
    )
}
