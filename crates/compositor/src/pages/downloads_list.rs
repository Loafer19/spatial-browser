// The Ctrl+J downloads-list page: most-recent-first, each row a
// favicon+filename+source-host, click to open the file with the
// desktop's default handler, an "x" to forget the entry (the file
// itself stays on disk — this only edits the list). See cef-bridge's
// OsrRequestHandler / app.rs's PENDING_DOWNLOAD_ACTION for how a row
// click here gets handled, and cef-bridge's OsrDownloadHandler for how
// a download lands under ~/Downloads in the first place.

use super::html_escape;
use crate::output::Theme;
use crate::persistence::bookmarks::host_of;
use crate::persistence::downloads::DownloadRecord;

/// Builds the downloads-list page's `data:` URL. `pub(crate)` so app.rs
/// can rebuild this page in place after a remove (same close+respawn
/// pattern as the bookmarks list — see refresh_bookmarks_page).
pub(crate) fn page_url(theme: &Theme, downloads: &[DownloadRecord]) -> String {
    let mut rows = String::new();
    if downloads.is_empty() {
        rows.push_str(&format!(
            "<p style=\"color:{fg};opacity:0.7\">No downloads yet.</p>",
            fg = theme.help_fg,
        ));
    }

    for (index, download) in downloads.iter().enumerate() {
        let filename = std::path::Path::new(&download.path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&download.path);
        let host = host_of(&download.url);
        let letter = filename.chars().next().unwrap_or('?').to_uppercase();
        rows.push_str(&format!(
            "<div class=\"dl-row\" onclick=\"location='download://open/{index}'\" \
             style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             cursor:pointer;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             <span style=\"position:relative;width:20px;height:20px;flex-shrink:0\">\
             <span style=\"position:absolute;inset:0;border-radius:4px;background:{key_bg};\
             color:{key_fg};display:flex;align-items:center;justify-content:center;\
             font-size:11px;font-weight:700\">{letter}</span></span>\
             <div style=\"flex:1;min-width:0;display:flex;flex-direction:column\">\
             <span style=\"overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
             color:{fg}\">{filename_html}</span>\
             <span style=\"overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
             color:{fg};opacity:0.6;font-size:12px\">{host_html}</span></div>\
             <a href=\"download://remove/{index}\" onclick=\"event.stopPropagation()\" \
             title=\"Remove from list\" class=\"bm-icon-btn\" style=\"flex-shrink:0;\
             display:flex;align-items:center;justify-content:center;width:26px;\
             height:26px;border-radius:6px;background:{bg};color:{fg};opacity:0.7;\
             text-decoration:none\">\
             <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
             <path d=\"M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z\"/>\
             </svg></a></div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            key_bg = theme.help_key_bg,
            key_fg = theme.help_key_fg,
            bg = theme.help_bg,
            fg = theme.help_fg,
            filename_html = html_escape(filename),
            host_html = html_escape(host),
        ));
    }

    format!(
        "data:text/html,<style>.dl-row:hover{{background:{key_bg}!important}}\
         .dl-row:hover span{{color:{key_fg}!important}}\
         .bm-icon-btn:hover{{background:{key_bg}!important;color:{key_fg}!important;\
         opacity:1!important}}</style>\
         <body style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <h1 style=\"margin:0 0 20px;color:{heading};font-size:20px\">Downloads</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        bg = theme.help_bg,
        fg = theme.help_fg,
        heading = theme.help_heading,
    )
}
