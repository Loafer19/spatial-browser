// Userscripts/userstyles list actions.

use super::*;

pub(super) fn refresh_userscripts_page(
    session: &mut Session,
    gpu: &GpuState,
    scripts: &[UserScript],
    styles: &[UserStyle],
    browser_id: i32,
) {
    let url = pages::userscripts_list::page_url(&session.theme(), scripts, styles);
    show_in_place(session, gpu, browser_id, &url);
}

/// `userscripts://` from the scripts & styles list.
pub(super) fn drain(
    session: &mut Session,
    gpu: &GpuState,
    userscripts: &mut Vec<UserScript>,
    userstyles: &mut Vec<UserStyle>,
) {
    if let Some((browser_id, action)) = PENDING_USERSCRIPT_ACTION
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "userscripts://"))
    {
        match action {
            UserscriptsPageAction::Reload => {
                userscripts::reload(userscripts);
                userstyles::reload(userstyles);
            }
            UserscriptsPageAction::OpenDir => {
                userscripts::open_dir();
            }
            UserscriptsPageAction::OpenStylesDir => {
                userstyles::open_dir();
            }
            UserscriptsPageAction::Toggle(name) => {
                userscripts::toggle_enabled(userscripts, &name);
            }
            UserscriptsPageAction::ToggleStyle(name) => {
                userstyles::toggle_enabled(userstyles, &name);
            }
        }
        refresh_userscripts_page(session, gpu, userscripts, userstyles, browser_id);
    }
}
