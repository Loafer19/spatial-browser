// Omnibox/switcher submits, launch URLs and popups → tiles.

use super::*;

/// `omnibox://go`: record the typed text, replace the omnibox tile with the destination.
pub(super) fn drain_omnibox(
    session: &mut Session,
    gpu: &GpuState,
    typed_history: &mut Vec<String>,
) {
    // omnibox://go → log typed text; close+respawn at resolved URL.
    if let Some((browser_id, submit)) = PENDING_OMNIBOX
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "omnibox://"))
    {
        typed_history::record(typed_history, &submit.raw);
        if let Some(index) = session
            .pages()
            .iter()
            .position(|p| p.browser.identifier() == browser_id)
        {
            if let Some(rect) = session.close_at(index) {
                session.add_page(browser::spawn(gpu, &gpu.window, &submit.url, rect, false));
            }
        }
    }
}

/// `switcher://go/{id}`: front and pan to the target, close the switcher.
pub(super) fn drain_switcher(session: &mut Session, gpu: &GpuState) {
    // switcher://go/{id} → front+pan target; close switcher (not undo stack).
    if let Some((switcher_id, target_id)) = PENDING_SWITCH
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "switcher://"))
    {
        if let Some(target_index) = session
            .pages()
            .iter()
            .position(|p| p.browser.identifier() == target_id)
        {
            session.bring_to_front(target_index);
            let rect = session
                .pages()
                .last()
                .expect("just brought a page to front")
                .rect;
            let viewport = session.viewport();
            let size = gpu.window.inner_size();
            let screen_center = (size.width as f32 / 2.0, size.height as f32 / 2.0);
            let world_center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            session.pan_viewport_to((
                world_center.0 - screen_center.0 / viewport.zoom,
                world_center.1 - screen_center.1 / viewport.zoom,
            ));
        }
        if let Some(switcher_index) = session
            .pages()
            .iter()
            .position(|p| p.browser.identifier() == switcher_id)
        {
            session.close_at(switcher_index);
        }
    }
}

/// Launch/second-launch URLs and accepted popups → new tiles.
pub(super) fn drain_new_pages(session: &mut Session, gpu: &GpuState) {
    // URLs from launch argv / a second launch → new pages, newest on top.
    for url in crate::single_instance::take_incoming() {
        let size = gpu.window.inner_size();
        let rect = session.cascade_rect((size.width as f32, size.height as f32));
        session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, false));
    }

    // Canceled popups → spawn canvas Page (cascaded from opener rect).
    let popups = PENDING_POPUPS.with_borrow_mut(std::mem::take);
    for (opener_id, url) in popups {
        let opener_rect = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == opener_id)
            .map(|p| p.rect);
        let size = gpu.window.inner_size();
        let rect = match opener_rect {
            Some(r) => Rect {
                x: r.x + 40.0,
                y: r.y + 40.0,
                w: r.w,
                h: r.h,
            },
            None => session.cascade_rect((size.width as f32, size.height as f32)),
        };
        session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, false));
    }
}
