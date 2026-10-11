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
            HistoryPageAction::RemoveAll(index) => {
                if let Some(url) = history.get(index).map(|e| e.url.clone()) {
                    if history::remove_url(history, &url) {
                        history::save(history);
                    }
                }
                refresh_history_page(session, gpu, history, browser_id);
            }
            HistoryPageAction::Search {
                query,
                offset,
                collapse,
            } => {
                send_search_results(session, browser_id, history, &query, offset, collapse);
            }
            HistoryPageAction::Clear => {
                history.clear();
                // The icon cache lists visited sites too.
                favicons::clear();
                history::save(history);
                refresh_history_page(session, gpu, history, browser_id);
            }
        }
    }
}

/// Answer `history://search` by script into the asking page. `drain` only
/// gets here for a built-in history tile (`from_internal`), so rows never
/// reach web content.
fn send_search_results(
    session: &Session,
    browser_id: i32,
    history: &[HistoryEntry],
    query: &str,
    offset: usize,
    collapse: bool,
) {
    let Some(page) = session
        .pages()
        .iter()
        .find(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let Some(frame) = page.browser.main_frame() else {
        return;
    };
    let json = pages::history_list::results_json(history, query, offset, collapse);
    let js = format!("window.histResults && histResults({json});");
    frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
}
