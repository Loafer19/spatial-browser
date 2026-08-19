// The Ctrl+Shift+W workspace-list page: each row a load button, a
// click-to-edit rename form, a page count, and a delete button; "Save
// current" at the top snapshots the canvas as a new entry. See
// cef-bridge's OsrRequestHandler / app.rs's PENDING_WORKSPACE_ACTION
// for how clicks on this page's `workspace://...` links get handled.

use super::html_escape;
use crate::output::Theme;
use crate::persistence::workspaces::Workspace;

/// Builds the workspace-list page's `data:` URL. `pub(crate)` so app.rs
/// can rebuild this page in place after a save/rename/delete (same
/// close+respawn pattern as the bookmarks list — see
/// refresh_bookmarks_page). Loading a workspace closes this page along
/// with everything else on the canvas, so there's no "refresh in
/// place" case for `Load` — only Save/Rename/Delete need one.
pub(crate) fn page_url(theme: &Theme, workspaces: &[Workspace]) -> String {
    let mut rows = String::new();
    if workspaces.is_empty() {
        rows.push_str(&format!(
            "<p style=\"color:{fg};opacity:0.7\">No saved workspaces yet.</p>",
            fg = theme.help_fg,
        ));
    }

    for (index, workspace) in workspaces.iter().enumerate() {
        let page_count = workspace.pages.len();
        let noun = if page_count == 1 { "page" } else { "pages" };
        rows.push_str(&format!(
            "<div style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             background:{card_bg};border-radius:8px;border:1px solid {card_border}\">\
             <a href=\"workspace://load/{index}\" title=\"Load\" class=\"bm-icon-btn\" \
             style=\"flex-shrink:0;display:flex;align-items:center;justify-content:center;\
             width:26px;height:26px;border-radius:6px;background:{bg};color:{fg};\
             opacity:0.7;text-decoration:none\">\
             <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
             <path d=\"M8 5v14l11-7z\"/></svg></a>\
             <form method=\"get\" action=\"workspace://rename/{index}\" \
             style=\"display:flex;align-items:center;gap:6px;flex:1;min-width:0;margin:0\">\
             <span onclick=\"this.style.display='none';\
             this.nextElementSibling.style.display='inline-block';\
             this.nextElementSibling.focus();this.nextElementSibling.select()\" \
             style=\"cursor:text;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
             flex:1;color:{fg}\" title=\"Click to rename\">{name}</span>\
             <input name=\"name\" value=\"{name_attr}\" \
             style=\"display:none;flex:1;min-width:0;background:{bg};color:{fg};\
             border:1px solid {card_border};border-radius:4px;padding:2px 6px;\
             font:inherit;font-size:13px\">\
             <button type=\"submit\" title=\"Save\" class=\"bm-icon-btn\" style=\"flex-shrink:0;\
             display:flex;align-items:center;justify-content:center;width:26px;height:26px;\
             border-radius:6px;background:{bg};color:{fg};opacity:0.7;border:none;\
             cursor:pointer;font:inherit\">\
             <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
             <path d=\"M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z\"/></svg></button>\
             </form>\
             <span style=\"flex-shrink:0;color:{fg};opacity:0.6;font-size:12px;\
             white-space:nowrap\">{page_count} {noun}</span>\
             <a href=\"workspace://delete/{index}\" title=\"Delete\" class=\"bm-icon-btn\" \
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
            bg = theme.help_bg,
            name = html_escape(&workspace.name),
            name_attr = html_escape(&workspace.name),
        ));
    }

    format!(
        "data:text/html,<style>.bm-icon-btn:hover{{background:{key_bg}!important;\
         color:{key_fg}!important;opacity:1!important}}</style>\
         <body style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <div style=\"display:flex;align-items:baseline;justify-content:space-between;\
         margin:0 0 20px\">\
         <h1 style=\"margin:0;color:{heading};font-size:20px\">Workspaces</h1>\
         <a href=\"workspace://save\" style=\"color:{fg};opacity:0.8;font-size:13px\">\
         + Save current</a>\
         </div>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        bg = theme.help_bg,
        fg = theme.help_fg,
        heading = theme.help_heading,
    )
}
