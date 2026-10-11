// Ctrl+K palette (`palette://` → PENDING_PALETTE_ACTION): go to an open
// tile, run a command on the tile that was focused when the palette opened,
// open a bookmark/history URL, or close. Typed URLs and searches take the
// omnibox's own `omnibox://go` path (spawning::drain_omnibox).

use super::*;
use crate::commands::{self, Command};
use cef_bridge::{PaletteAction, PENDING_PALETTE_ACTION};

/// The open palette tile and the tile focused when it opened.
#[derive(Clone, Copy)]
struct OpenPalette {
    palette_id: i32,
    target_id: Option<i32>,
}

thread_local! {
    static PALETTE: Cell<Option<OpenPalette>> = const { Cell::new(None) };
}

/// The palette tile `palette_id` just opened over `target_id`.
pub(crate) fn palette_opened(palette_id: i32, target_id: Option<i32>) {
    PALETTE.set(Some(OpenPalette {
        palette_id,
        target_id,
    }));
}

/// Close the open palette tile, if it is in this session. True if it was.
pub(crate) fn close_palette(session: &mut Session) -> bool {
    PALETTE
        .get()
        .is_some_and(|open| close_tile(session, open.palette_id))
}

fn close_tile(session: &mut Session, browser_id: i32) -> bool {
    if PALETTE
        .get()
        .is_some_and(|open| open.palette_id == browser_id)
    {
        PALETTE.set(None);
    }
    match session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    {
        Some(index) => session.close_at(index).is_some(),
        None => false,
    }
}

/// The queued action, if a built-in tile sent it (web pages can navigate
/// to `palette://` too; theirs are dropped here).
fn take_action(session: &Session) -> Option<(i32, PaletteAction)> {
    PENDING_PALETTE_ACTION
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "palette://"))
}

pub(super) fn drain(session: &mut Session, gpu: &GpuState, s: &mut Stores<'_>) {
    let Some((palette_id, action)) = take_action(session) else {
        return;
    };
    let target = PALETTE
        .get()
        .filter(|open| open.palette_id == palette_id)
        .and_then(|open| open.target_id);
    // Whatever was picked, the palette is done.
    close_tile(session, palette_id);
    match action {
        PaletteAction::Close => {}
        PaletteAction::Go(id) => front_and_pan(session, gpu, id),
        PaletteAction::Open(url) => {
            let size = gpu.window.inner_size();
            let rect = session.cascade_rect((size.width as f32, size.height as f32));
            session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, false));
        }
        PaletteAction::Run(id) => {
            let Some(command) = Command::from_id(&id) else {
                log::warn!("unknown palette command {id:?}");
                return;
            };
            let index = target.and_then(|t| {
                session
                    .pages()
                    .iter()
                    .position(|p| p.browser.identifier() == t)
            });
            match index {
                // Page commands act on the topmost tile: make it the one
                // that was focused before the palette opened.
                Some(index) => {
                    session.bring_to_front(index);
                }
                None if command.targets_page() => {
                    log::info!("palette command {id} skipped: its page is gone");
                    return;
                }
                None => {}
            }
            commands::run(command, session, gpu, s);
        }
    }
}

/// Front the tile `target_id` and pan the canvas to centre it.
fn front_and_pan(session: &mut Session, gpu: &GpuState, target_id: i32) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == target_id)
    else {
        return;
    };
    let index = session.bring_to_front(index);
    let rect = session.pages()[index].rect;
    let viewport = session.viewport();
    let size = gpu.window.inner_size();
    let screen_center = (size.width as f32 / 2.0, size.height as f32 / 2.0);
    let world_center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    session.pan_viewport_to((
        world_center.0 - screen_center.0 / viewport.zoom,
        world_center.1 - screen_center.1 / viewport.zoom,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::Viewport;

    #[test]
    fn run_from_a_non_internal_browser_is_dropped() {
        // No tile in the session holds an issued token for browser 4242,
        // which is exactly what a web page's navigation looks like here.
        let session = Session::new(Vec::new(), Viewport::default(), THEMES[0]);
        for action in [
            PaletteAction::Run("close-page".into()),
            PaletteAction::Go(1),
            PaletteAction::Open("https://evil.test/".into()),
            PaletteAction::Close,
        ] {
            PENDING_PALETTE_ACTION.with_borrow_mut(|p| *p = Some((4242, action)));
            assert!(take_action(&session).is_none());
            // Dropped, not left queued for a later (internal) drain.
            assert!(PENDING_PALETTE_ACTION.with_borrow(|p| p.is_none()));
        }
    }

    #[test]
    fn close_palette_without_a_tile_is_a_no_op() {
        let mut session = Session::new(Vec::new(), Viewport::default(), THEMES[0]);
        assert!(!close_palette(&mut session));
        palette_opened(77, Some(1));
        // Parked in another workspace: not here, nothing to close, and the
        // record of it goes once asked about.
        assert!(!close_palette(&mut session));
        assert!(PALETTE.get().is_none());
    }
}
