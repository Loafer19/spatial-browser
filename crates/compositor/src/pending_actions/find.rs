// Ctrl+F find bar.

use super::*;

/// Ctrl+F bar: its `find://` actions, CEF's match counts, and keeping the
/// bar glued above its page (or tearing both down when either is gone).
pub(super) fn apply_find(session: &mut Session, gpu: &GpuState) {
    let action = PENDING_FIND_ACTION.with_borrow_mut(|pending| pending.take());
    let results = PENDING_FIND_RESULTS.with_borrow_mut(std::mem::take);
    let Some(find) = session.find() else {
        return;
    };
    let (bar_id, target_id) = (find.bar_id, find.target_id);
    let index_of =
        |s: &Session, id: i32| s.pages().iter().position(|p| p.browser.identifier() == id);
    let host_of = |s: &Session, id: i32| {
        s.pages()
            .iter()
            .find(|p| p.browser.identifier() == id)
            .and_then(|p| p.browser.host())
    };

    let bar_index = index_of(session, bar_id);
    let target_rect = index_of(session, target_id).map(|i| session.pages()[i].rect);
    let is_close = matches!(action, Some((id, FindAction::Close)) if id == bar_id);
    if is_close || bar_index.is_none() || target_rect.is_none() {
        if let Some(host) = host_of(session, target_id) {
            host.stop_finding(true as _);
        }
        if let Some(index) = bar_index {
            session.close_at(index);
        }
        session.set_find(None);
        return;
    }

    if let Some((
        id,
        FindAction::Search {
            text,
            forward,
            match_case,
        },
    )) = action
    {
        if id == bar_id && from_internal(session, id, "find://") {
            if let Some(host) = host_of(session, target_id) {
                let query = (text.clone(), match_case);
                let is_repeat = session.find().and_then(|f| f.last_query.as_ref()) == Some(&query);
                if text.is_empty() {
                    host.stop_finding(true as _);
                } else {
                    host.find(
                        Some(&text.as_str().into()),
                        forward as _,
                        match_case as _,
                        is_repeat as _,
                    );
                }
                if let Some(find) = session.find_mut() {
                    find.last_query = (!text.is_empty()).then_some(query);
                }
            }
        }
    }

    if let Some(result) = results
        .iter()
        .rev()
        .find(|(id, _)| *id == target_id)
        .map(|(_, r)| *r)
    {
        if let Some(frame) = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == bar_id)
            .and_then(|p| p.browser.main_frame())
        {
            let js = format!(
                "window.__found && window.__found({}, {});",
                result.active.max(0),
                result.count.max(0)
            );
            frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
        }
    }

    if let Some(rect) = target_rect {
        let scale = gpu.window.scale_factor();
        session.place_page(bar_id, crate::session::find_bar_rect(rect, scale), scale);
    }
}
