// The Ctrl+B bookmarks-list page: grouped by folder, each row a
// favicon+label, a click-to-edit rename form, and a delete button. See
// cef-bridge's OsrRequestHandler / app.rs's PENDING_BOOKMARK for how
// clicks on this page's `bookmark://...` links get handled.

use super::html_escape;
use crate::output::Theme;
use crate::persistence::bookmarks::{self, Bookmark};

/// Builds the bookmarks-list page's `data:` URL — grouped by folder
/// (ungrouped entries first, then each named folder in order of first
/// appearance). Each row is a real `<a href="bookmark://open/{index}">`
/// plus a delete link and a rename form; all three are normal
/// navigations that CEF's `on_before_browse` (cef-bridge) intercepts and
/// cancels, signaling the action back to the compositor
/// (`app.rs`'s `PENDING_BOOKMARK` handling) instead of actually loading
/// `bookmark://...`. `pub(crate)` so app.rs can rebuild this page in
/// place after a delete/rename.
///
/// The favicon is fetched live from the bookmark's own site
/// (`https://{host}/favicon.ico`) rather than captured/stored ourselves;
/// a colored initial-letter tile sits underneath it as a fallback that
/// never needs the network, shown until (or unless) the real icon loads.
pub(crate) fn page_url(theme: &Theme, bookmarks: &[Bookmark]) -> String {
    let mut rows = String::new();
    if bookmarks.is_empty() {
        rows.push_str(&format!(
            "<p style=\"color:{fg};opacity:0.7\">No bookmarks yet &mdash; Ctrl+D on a page to add one.</p>",
            fg = theme.help_fg,
        ));
    }

    let mut folders: Vec<Option<&str>> = Vec::new();
    for bookmark in bookmarks {
        let folder = bookmark.folder.as_deref();
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }

    for folder in folders {
        if let Some(name) = folder {
            rows.push_str(&format!(
                "<h2 style=\"margin:12px 0 0;font-size:13px;text-transform:uppercase;\
                 letter-spacing:0.05em;color:{heading};opacity:0.8\">{name}</h2>",
                heading = theme.help_heading,
                name = html_escape(name),
            ));
        }
        for (index, bookmark) in bookmarks.iter().enumerate() {
            if bookmark.folder.as_deref() != folder {
                continue;
            }
            let host = bookmarks::host_of(&bookmark.url);
            let label = bookmarks::display_label(bookmark);
            let letter = label.chars().next().unwrap_or('?').to_uppercase();
            rows.push_str(&format!(
                "<div style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
                 background:{card_bg};border-radius:8px;border:1px solid {card_border}\">\
                 <a href=\"bookmark://open/{index}\" style=\"display:flex;flex-shrink:0\">\
                 <span style=\"position:relative;width:20px;height:20px\">\
                 <span style=\"position:absolute;inset:0;border-radius:4px;background:{key_bg};\
                 color:{key_fg};display:flex;align-items:center;justify-content:center;\
                 font-size:11px;font-weight:700\">{letter}</span>\
                 <img src=\"https://{host}/favicon.ico\" width=\"20\" height=\"20\" \
                 style=\"position:absolute;inset:0;border-radius:4px\" \
                 onerror=\"this.style.display='none'\"></span></a>\
                 <form method=\"get\" action=\"bookmark://rename/{index}\" \
                 style=\"display:flex;align-items:center;gap:6px;flex:1;min-width:0;margin:0\">\
                 <span onclick=\"this.style.display='none';\
                 this.nextElementSibling.style.display='inline-block';\
                 this.nextElementSibling.focus();this.nextElementSibling.select()\" \
                 style=\"cursor:text;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
                 flex:1;color:{fg}\" title=\"Click to rename\">{label}</span>\
                 <input name=\"title\" value=\"{label_attr}\" \
                 style=\"display:none;flex:1;min-width:0;background:{bg};color:{fg};\
                 border:1px solid {card_border};border-radius:4px;padding:2px 6px;\
                 font:inherit;font-size:13px\">\
                 <input name=\"folder\" value=\"{folder_attr}\" placeholder=\"folder\" \
                 style=\"width:70px;flex-shrink:0;background:{bg};color:{fg};\
                 border:1px solid {card_border};border-radius:4px;padding:2px 6px;\
                 font:inherit;font-size:12px\">\
                 <button type=\"submit\" title=\"Save\" class=\"bm-icon-btn\" style=\"flex-shrink:0;\
                 margin-left:4px;display:flex;align-items:center;justify-content:center;\
                 width:26px;height:26px;border-radius:6px;background:{bg};color:{fg};\
                 opacity:0.7;border:none;cursor:pointer;font:inherit\">\
                 <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
                 <path d=\"M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z\"/></svg></button>\
                 </form>\
                 <a href=\"bookmark://delete/{index}\" title=\"Delete\" class=\"bm-icon-btn\" \
                 style=\"flex-shrink:0;margin-left:4px;display:flex;align-items:center;\
                 justify-content:center;width:26px;height:26px;border-radius:6px;\
                 background:{bg};color:{fg};opacity:0.7;text-decoration:none\">\
                 <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
                 <path d=\"M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z\"/>\
                 </svg></a>\
                 </div>",
                card_bg = theme.help_card_bg,
                card_border = theme.help_card_border,
                fg = theme.help_fg,
                key_bg = theme.help_key_bg,
                key_fg = theme.help_key_fg,
                bg = theme.help_bg,
                label_attr = html_escape(label),
                folder_attr = html_escape(bookmark.folder.as_deref().unwrap_or("")),
            ));
        }
    }

    format!(
        "data:text/html,<style>.bm-icon-btn:hover{{background:{key_bg}!important;\
         color:{key_fg}!important;opacity:1!important}}</style>\
         <body style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <h1 style=\"margin:0 0 20px;color:{heading};font-size:20px\">Bookmarks</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        bg = theme.help_bg,
        fg = theme.help_fg,
        heading = theme.help_heading,
    )
}
