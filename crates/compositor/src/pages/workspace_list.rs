// Ctrl+Shift+W workspace-slot list: switch / delete / add. Live slots
// (not named snapshots). See workspaces.rs and PENDING_WORKSPACE_ACTION.

use super::{LIST_NAV_SCRIPT, TRASH_SVG_PATH};
use crate::output::Theme;
use crate::persistence::workspaces::WorkspaceStore;

const LOAD_SVG_PATH: &str = "M8 5v14l11-7z";

pub(crate) fn page_url(theme: &Theme, store: &WorkspaceStore) -> String {
    let mut rows = String::new();
    for (index, slot) in store.slots.iter().enumerate() {
        let active = slot.id == store.active;
        let status = if !slot.visited {
            "empty".to_string()
        } else {
            let n = slot.pages.len();
            if n == 1 {
                "1 page".into()
            } else {
                format!("{n} pages")
            }
        };
        let badge = if active {
            format!(
                "<span style=\"flex-shrink:0;font-size:11px;padding:2px 6px;border-radius:4px;\
                 background:{key_bg};color:{key_fg}\">active</span>",
                key_bg = theme.help_key_bg,
                key_fg = theme.help_key_fg,
            )
        } else {
            String::new()
        };
        let delete = if store.slots.len() > 1 {
            super::icon_link_button(
                &format!("workspace://delete/{index}"),
                "Delete",
                TRASH_SVG_PATH,
                "",
                theme,
            )
        } else {
            String::new()
        };
        rows.push_str(&format!(
            "<div class=\"list-row\" data-open=\"workspace://load/{index}\" \
             style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
             background:{card_bg};border-radius:8px;border:1px solid {card_border}\">\
             {load_button}\
             <span style=\"flex:1;color:{fg};font-weight:600\">Workspace {id}</span>\
             <span style=\"flex-shrink:0;color:{fg};opacity:0.6;font-size:12px\">{status}</span>\
             {badge}{delete}\
             </div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            load_button = super::icon_link_button(
                &format!("workspace://load/{index}"),
                "Switch",
                LOAD_SVG_PATH,
                "",
                theme,
            ),
            fg = theme.help_fg,
            id = slot.id,
            status = status,
            badge = badge,
            delete = delete,
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
         + New workspace</a>\
         </div>\
         <p style=\"margin:0 0 12px;color:{fg};opacity:0.7;font-size:13px\">\
         Hover the top edge for chips · Ctrl+1–9 switch · Ctrl+N new</p>\
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
