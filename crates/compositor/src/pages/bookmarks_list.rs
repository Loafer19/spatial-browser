// The Ctrl+B bookmarks-list page: grouped by folder, each row a
// favicon+label, a click-to-edit rename form, and a delete button. See
// cef-bridge's OsrRequestHandler / app.rs's PENDING_BOOKMARK for how
// clicks on this page's `bookmark://...` links get handled.

use super::{html_escape, CHECKMARK_SVG_PATH, LIST_NAV_SCRIPT, TRASH_SVG_PATH};
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
        rows.push_str(&super::empty_state(
            theme,
            "No bookmarks yet &mdash; Ctrl+D on a page to add one.",
        ));
    }

    let mut folders: Vec<Option<&str>> = Vec::new();
    for bookmark in bookmarks {
        let folder = bookmark.folder.as_deref();
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    // `Option<&str>`'s own Ord already does exactly what's wanted here:
    // `None` sorts before every `Some`, so ungrouped bookmarks always
    // render first, and the named folders after it sort alphabetically
    // (comparing their `&str` contents) rather than by first appearance.
    folders.sort();

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
            let letter = label
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            rows.push_str(&format!(
                "<div class=\"list-row\" data-open=\"bookmark://open/{index}\" \
                 style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
                 background:{card_bg};border-radius:8px;border:1px solid {card_border}\">\
                 <a href=\"bookmark://open/{index}\" style=\"display:flex;flex-shrink:0\">{favicon}</a>\
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
                 {save_button}\
                 </form>\
                 {delete_button}\
                 </div>",
                card_bg = theme.help_card_bg,
                card_border = theme.help_card_border,
                favicon = super::favicon_tile(host, &letter, theme),
                fg = theme.help_fg,
                bg = theme.help_bg,
                label_attr = html_escape(label),
                folder_attr = html_escape(bookmark.folder.as_deref().unwrap_or("")),
                save_button = super::icon_submit_button("Save", CHECKMARK_SVG_PATH, theme),
                delete_button = super::icon_link_button(
                    &format!("bookmark://delete/{index}"),
                    "Delete",
                    TRASH_SVG_PATH,
                    "",
                    theme,
                ),
            ));
        }
    }

    format!(
        "data:text/html;charset=utf-8,{nav_script}\
         <style>{icon_hover}\
         .list-row:hover,.list-row.list-active{{background:{key_bg}!important}}\
         .list-row:hover span,.list-row.list-active span{{color:{key_fg}!important}}</style>\
         {body_open}\
         <h1 style=\"margin:0 0 20px;color:{heading};font-size:20px\">Bookmarks</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        nav_script = LIST_NAV_SCRIPT,
        icon_hover = super::icon_button_hover_css(theme),
        body_open = super::body_open(theme, ""),
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        heading = theme.help_heading,
    )
}
