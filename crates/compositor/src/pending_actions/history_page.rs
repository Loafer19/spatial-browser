// History list actions.

use super::*;

/// Like `refresh_bookmarks_page`, for history.
pub(super) fn refresh_history_page(
    session: &mut Session,
    gpu: &GpuState,
    history: &[HistoryEntry],
    browser_id: i32,
) {
    let url = pages::history_list::page_url(&session.theme(), history);
    show_in_place(session, gpu, browser_id, &url);
}

/// `history://` from the history list.
pub(super) fn drain(session: &mut Session, gpu: &GpuState, history: &mut Vec<HistoryEntry>) {
    // history://…
    if let Some((browser_id, action)) = PENDING_HISTORY_ACTION
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "history://"))
    {
        match action {
            HistoryPageAction::Open(index) => {
                if let Some(entry) = history.get(index) {
                    let size = gpu.window.inner_size();
                    let rect = session.cascade_rect((size.width as f32, size.height as f32));
                    session.add_page(browser::spawn(gpu, &gpu.window, &entry.url, rect, false));
                }
            }
            HistoryPageAction::Remove(index) => {
                if index < history.len() {
                    history.remove(index);
                    history::save(history);
                }
                refresh_history_page(session, gpu, history, browser_id);
            }
            HistoryPageAction::Clear => {
                history.clear();
                history::save(history);
                refresh_history_page(session, gpu, history, browser_id);
            }
        }
    }
}
