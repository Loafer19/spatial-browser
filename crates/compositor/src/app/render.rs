// RedrawRequested: build page draws, HUD and minimap, present.

use super::*;

impl App {
    /// `WindowEvent::RedrawRequested`.
    pub(super) fn on_redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = &mut self.state else {
            return;
        };
        // Begin frames, pending CEF actions, cursor and session
        // saves run in `tick` (every loop iteration), not per frame.
        let viewport = self.session.viewport();
        let pages = self.session.pages();
        let last_index = pages.len().saturating_sub(1);
        let textures: Vec<_> = pages.iter().map(Page::texture).collect();
        let draws: Vec<PageDraw> = pages
            .iter()
            .zip(textures.iter())
            .enumerate()
            .map(|(i, (page, texture))| {
                let progress = page.load_progress.get();
                let active = page.load_active.get() && !page.ephemeral;
                PageDraw {
                    rect: viewport.rect_to_screen(page.rect),
                    quad: &page.quad,
                    texture: texture.as_ref(),
                    focused: i == last_index,
                    load_progress: progress,
                    load_bar: active && progress < 0.999,
                }
            })
            .collect();

        let theme = self.session.theme();
        let overlay = crate::perf::shown().then(|| {
            let path = if crate::output::osr_shared_texture_enabled() {
                "gpu paint"
            } else {
                "cpu paint"
            };
            self.perf.lines(path)
        });
        if let Some(hud) = &mut self.hud {
            hud.rebuild(
                &state.queue,
                &state.device,
                &self.workspaces,
                &theme,
                overlay.as_deref(),
            );
        }
        let focused = self.session.pages().len().checked_sub(1);
        if let Some(minimap) = &mut self.minimap {
            minimap.rebuild(
                &state.queue,
                &state.device,
                self.session.pages(),
                &viewport,
                &theme,
                focused,
            );
        }
        let hud_ref = self.hud.as_ref();
        let minimap_ref = self.minimap.as_ref();
        let started = Instant::now();
        let outcome = state.render(
            &draws,
            &theme,
            viewport.offset,
            viewport.zoom,
            hud_ref,
            minimap_ref,
        );
        match outcome {
            FrameOutcome::Rendered => {
                let now = Instant::now();
                self.perf.frame_presented(now - started);
                self.pacer.rendered(now);
            }
            FrameOutcome::Skip => self.pacer.render_skipped(Instant::now()),
            FrameOutcome::Reconfigure => {
                let size = state.window.inner_size();
                state.resize(size.width, size.height);
                self.pacer.render_skipped(Instant::now());
            }
            FrameOutcome::Fatal => {
                log::error!("fatal surface validation error, exiting");
                event_loop.exit();
            }
        }
    }
}
