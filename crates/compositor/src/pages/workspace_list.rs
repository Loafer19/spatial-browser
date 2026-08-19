// The Ctrl+Shift+W workspace-list page: each row a load button, a
// click-to-edit rename form, a page count, and a delete button; "Save
// current" at the top snapshots the canvas as a new entry. See
// cef-bridge's OsrRequestHandler / app.rs's PENDING_WORKSPACE_ACTION
// for how clicks on this page's `workspace://...` links get handled.

use super::{html_escape, CHECKMARK_SVG_PATH, LIST_NAV_SCRIPT, TRASH_SVG_PATH};
use crate::output::Theme;
use crate::persistence::workspaces::Workspace;

// Play-triangle SVG path for this page's own "Load" button — the one
// icon-button shape none of the other list pages need.
const LOAD_SVG_PATH: &str = "M8 5v14l11-7z";

/// Builds the workspace-list page's `data:` URL. `pub(crate)` so app.rs
/// can rebuild this page in place after a save/rename/delete (same
/// close+respawn pattern as the bookmarks list — see
/// refresh_bookmarks_page). Loading a workspace closes this page along
/// with everything else on the canvas, so there's no "refresh in
/// place" case for `Load` — only Save/Rename/Delete need one.
pub(crate) fn page_url(theme: &Theme, workspaces: &[Workspace]) -> String {
    let mut rows = String::new();
    if workspaces.is_empty() {
        rows.push_str(&super::empty_state(theme, "No saved workspaces yet."));
    }

    for (index, workspace) in workspaces.iter().enumerate() {
        let page_count = workspace.pages.len();
        let noun = if page_count == 1 { "page" } else { "pages" };
        rows.push_str(&format!(
            "<div class=\"list-row\" data-open=\"workspace://load/{index}\" \
             style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             background:{card_bg};border-radius:8px;border:1px solid {card_border}\">\
             {load_button}\
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
             {save_button}\
             </form>\
             <span style=\"flex-shrink:0;color:{fg};opacity:0.6;font-size:12px;\
             white-space:nowrap\">{page_count} {noun}</span>\
             {delete_button}\
             </div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            load_button = super::icon_link_button(
                &format!("workspace://load/{index}"),
                "Load",
                LOAD_SVG_PATH,
                "",
                theme,
            ),
            fg = theme.help_fg,
            bg = theme.help_bg,
            name = html_escape(&workspace.name),
            name_attr = html_escape(&workspace.name),
            save_button = super::icon_submit_button("Save", CHECKMARK_SVG_PATH, theme),
            delete_button = super::icon_link_button(
                &format!("workspace://delete/{index}"),
                "Delete",
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
         <div style=\"display:flex;align-items:baseline;justify-content:space-between;\
         margin:0 0 20px\">\
         <h1 style=\"margin:0;color:{heading};font-size:20px\">Workspaces</h1>\
         <a href=\"workspace://save\" style=\"color:{fg};opacity:0.8;font-size:13px\">\
         + Save current</a>\
         </div>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        nav_script = LIST_NAV_SCRIPT,
        icon_hover = super::icon_button_hover_css(theme),
        body_open = super::body_open(theme, ""),
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        fg = theme.help_fg,
        heading = theme.help_heading,
    )
}
