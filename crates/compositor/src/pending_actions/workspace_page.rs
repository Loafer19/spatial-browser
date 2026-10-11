// Workspace list actions.

use super::*;

/// Like `refresh_bookmarks_page`, for workspaces (not used for Load).
pub(super) fn refresh_workspaces_page(
    session: &mut Session,
    gpu: &GpuState,
    workspaces: &WorkspaceStore,
    browser_id: i32,
) {
    let url = pages::workspace_list::page_url(&session.theme(), workspaces);
    show_in_place(session, gpu, browser_id, &url);
}

/// `workspace://` from the workspace list.
pub(super) fn drain(
    session: &mut Session,
    gpu: &GpuState,
    workspaces: &mut WorkspaceStore,
    workspace_runtime: &mut WorkspaceRuntime,
) {
    // workspace://…
    if let Some((browser_id, action)) = PENDING_WORKSPACE_ACTION
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "workspace://"))
    {
        match action {
            WorkspacePageAction::Load(index) => {
                if let Some(slot) = workspaces.slots.get(index).cloned() {
                    workspaces::switch_to(workspaces, workspace_runtime, session, gpu, slot.id);
                }
            }
            WorkspacePageAction::Rename(index, name) => {
                if workspaces.rename(index, &name) {
                    // Snapshot first so the save doesn't write stale pages
                    // for the active slot.
                    workspaces.capture_active_from_session(session);
                    workspaces.save();
                }
                refresh_workspaces_page(session, gpu, workspaces, browser_id);
            }
            WorkspacePageAction::Delete(index) => {
                if let Some(id) = workspaces.slots.get(index).map(|s| s.id) {
                    workspaces::delete_slot(workspaces, workspace_runtime, session, gpu, id);
                }
                // List page may still be open if a non-active slot was deleted.
                if session
                    .pages()
                    .iter()
                    .any(|p| p.browser.identifier() == browser_id)
                {
                    refresh_workspaces_page(session, gpu, workspaces, browser_id);
                }
            }
            WorkspacePageAction::SaveNew => {
                workspaces::add_and_switch(workspaces, workspace_runtime, session, gpu);
            }
        }
    }
}
