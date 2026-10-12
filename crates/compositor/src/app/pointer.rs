// Pointer input: page drag/resize, canvas pan, minimap, context menu, touch.

use super::*;
use winit::dpi::PhysicalPosition;
use winit::event::{MouseScrollDelta, Touch};

/// Screen-space resize grab size at the bottom-right corner.
pub(super) const RESIZE_HANDLE: f32 = 18.0;
pub(super) const MIN_PAGE_SIZE: f32 = 160.0;

/// Topmost page whose bottom-right corner is within `RESIZE_HANDLE`.
pub(super) fn resize_hit_test(
    pages: &[Page],
    viewport: &Viewport,
    cursor_screen: (f32, f32),
) -> Option<usize> {
    let half = RESIZE_HANDLE / 2.0;
    pages.iter().rposition(|p| {
        let r = viewport.rect_to_screen(p.rect);
        let corner = (r.x + r.w, r.y + r.h);
        (cursor_screen.0 - corner.0).abs() <= half && (cursor_screen.1 - corner.1).abs() <= half
    })
}

/// Topmost page (last in z-order) whose rect contains the point, if any.
pub(super) fn hit_test(pages: &[Page], x: f32, y: f32) -> Option<usize> {
    pages.iter().rposition(|p| p.rect.contains(x, y))
}

impl App {
    /// `WindowEvent::CursorMoved`.
    pub(super) fn on_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let Some(state) = &mut self.state else {
            return;
        };
        let scale = state.window.scale_factor();
        self.cursor_window = (position.x as f32, position.y as f32);
        if let Some(hud) = &mut self.hud {
            hud.on_cursor(self.cursor_window.1);
        }

        if let Some((start, offset_at_start)) = self.minimap_drag {
            if let Some(minimap) = &self.minimap {
                if let Some(map_scale) = minimap.scale() {
                    let dx = self.cursor_window.0 - start.0;
                    let dy = self.cursor_window.1 - start.1;
                    // Dragging the viewfinder right → look further right.
                    self.session.pan_viewport_to((
                        offset_at_start.0 + dx / map_scale,
                        offset_at_start.1 + dy / map_scale,
                    ));
                }
            }
            return;
        }

        if let Some((start, offset_at_start)) = self.panning {
            let dx = self.cursor_window.0 - start.0;
            let dy = self.cursor_window.1 - start.1;
            let zoom = self.session.viewport().zoom;
            self.session
                .pan_viewport_to((offset_at_start.0 - dx / zoom, offset_at_start.1 - dy / zoom));
            return;
        }

        let viewport = self.session.viewport();
        let cursor_world = viewport.screen_to_world(self.cursor_window);
        self.resize_hover = self.resizing.is_some()
            || resize_hit_test(self.session.pages(), &viewport, self.cursor_window).is_some();

        if let Some(browser_id) = self.resizing {
            // Page closed mid-resize → end the gesture.
            let Some(page_rect) = self.session.rect_of(browser_id) else {
                self.resizing = None;
                return;
            };
            let rect = Rect {
                x: page_rect.x,
                y: page_rect.y,
                w: (cursor_world.0 - page_rect.x).max(MIN_PAGE_SIZE),
                h: (cursor_world.1 - page_rect.y).max(MIN_PAGE_SIZE),
            };
            self.session.set_rect_of(browser_id, rect, scale);
            return;
        }

        if let Some((browser_id, offset)) = self.dragging {
            // Page closed mid-drag → end the gesture.
            let Some(page_rect) = self.session.rect_of(browser_id) else {
                self.dragging = None;
                return;
            };
            let rect = Rect {
                x: cursor_world.0 - offset.0,
                y: cursor_world.1 - offset.1,
                w: page_rect.w,
                h: page_rect.h,
            };
            self.session.set_rect_of(browser_id, rect, scale);
            return;
        }

        if let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1) {
            let page = &self.session.pages()[i];
            let local = (
                (cursor_world.0 - page.rect.x) as f64,
                (cursor_world.1 - page.rect.y) as f64,
            );
            let host = page.browser.host();
            self.mouse.cursor_moved(local, scale, host.as_ref());
        }
    }

    /// `WindowEvent::MouseInput`.
    pub(super) fn on_mouse_input(&mut self, element_state: ElementState, button: MouseButton) {
        let Some(state) = &mut self.state else {
            return;
        };
        if element_state == ElementState::Pressed {
            let world = self.session.viewport().screen_to_world(self.cursor_window);
            let hit = hit_test(self.session.pages(), world.0, world.1)
                .map(|i| self.session.pages()[i].browser.identifier());
            crate::pending_actions::dismiss_palette_on_press(&mut self.session, hit);
        }
        // Middle-drag pans (works with a real mouse); Shift+Left-drag
        // is the trackpad-friendly equivalent — most touchpads have
        // no reliable middle button or a sustained right-click-drag
        // gesture, but a plain click+drag with a held modifier works
        // everywhere, same as Alt+Left-drag below for moving a page.
        if button == MouseButton::Middle
            || (button == MouseButton::Left && self.modifiers.shift_key())
        {
            match element_state {
                ElementState::Pressed => {
                    self.panning = Some((self.cursor_window, self.session.viewport().offset));
                }
                ElementState::Released => self.panning = None,
            }
            return;
        }

        if button == MouseButton::Left && self.modifiers.alt_key() {
            let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
            match element_state {
                ElementState::Pressed => {
                    if let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1)
                    {
                        let top = self.session.bring_to_front(i);
                        let page = &self.session.pages()[top];
                        let rect = page.rect;
                        self.dragging = Some((
                            page.browser.identifier(),
                            (cursor_world.0 - rect.x, cursor_world.1 - rect.y),
                        ));
                    }
                }
                ElementState::Released => self.dragging = None,
            }
            return;
        }

        if button == MouseButton::Left {
            match element_state {
                ElementState::Pressed => {
                    if let Some(hud) = &self.hud {
                        if let Some(hit) = hud.hit_test(
                            &self.workspaces,
                            self.cursor_window.0,
                            self.cursor_window.1,
                        ) {
                            match hit {
                                HudHit::Slot(id) => {
                                    workspaces::switch_to(
                                        &mut self.workspaces,
                                        &mut self.workspace_runtime,
                                        &mut self.session,
                                        state,
                                        id,
                                    );
                                }
                                HudHit::Add => {
                                    workspaces::add_and_switch(
                                        &mut self.workspaces,
                                        &mut self.workspace_runtime,
                                        &mut self.session,
                                        state,
                                    );
                                }
                            }
                            return;
                        }
                    }
                    if let Some(minimap) = &self.minimap {
                        if minimap.visible()
                            && minimap.contains_screen(self.cursor_window.0, self.cursor_window.1)
                        {
                            if minimap.hit_viewfinder(self.cursor_window.0, self.cursor_window.1) {
                                self.minimap_drag =
                                    Some((self.cursor_window, self.session.viewport().offset));
                            } else if let Some(world) =
                                minimap.screen_to_world(self.cursor_window.0, self.cursor_window.1)
                            {
                                let (sw, sh) = self.canvas_size;
                                let zoom = self.session.viewport().zoom;
                                self.session.pan_viewport_to((
                                    world.0 - sw / zoom * 0.5,
                                    world.1 - sh / zoom * 0.5,
                                ));
                            }
                            return;
                        }
                    }
                    // Dismiss context menu on any left click outside it
                    // (clicks on the menu itself are CEF navigations).
                    if self.context_menu.is_some() {
                        let cursor_world =
                            self.session.viewport().screen_to_world(self.cursor_window);
                        let on_menu = self.context_menu.as_ref().and_then(|cm| {
                            self.session.pages().iter().find(|p| {
                                p.browser.identifier() == cm.menu_browser_id
                                    && cursor_world.0 >= p.rect.x
                                    && cursor_world.0 <= p.rect.x + p.rect.w
                                    && cursor_world.1 >= p.rect.y
                                    && cursor_world.1 <= p.rect.y + p.rect.h
                            })
                        });
                        if on_menu.is_none() {
                            pending_actions::dismiss_context_menu(
                                &mut self.session,
                                &mut self.context_menu,
                            );
                        }
                    }
                    if let Some(i) = resize_hit_test(
                        self.session.pages(),
                        &self.session.viewport(),
                        self.cursor_window,
                    ) {
                        let top = self.session.bring_to_front(i);
                        self.resizing = Some(self.session.pages()[top].browser.identifier());
                        return;
                    }
                }
                ElementState::Released => {
                    if self.minimap_drag.take().is_some() {
                        return;
                    }
                    if self.resizing.take().is_some() {
                        return;
                    }
                }
            }
        }

        // Right-click → our context menu (not CEF's).
        if button == MouseButton::Right && element_state == ElementState::Pressed {
            pending_actions::dismiss_context_menu(&mut self.session, &mut self.context_menu);
            let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
            let scale = state.window.scale_factor();
            match hit_test(self.session.pages(), cursor_world.0, cursor_world.1) {
                None => {
                    pending_actions::open_context_menu(
                        &mut self.session,
                        state,
                        &mut self.context_menu,
                        self.cursor_window,
                        crate::pages::context_menu::MenuContext {
                            on_canvas: true,
                            ..Default::default()
                        },
                    );
                }
                Some(i) => {
                    let i = self.session.bring_to_front(i);
                    let page = &self.session.pages()[i];
                    // Skip reopening menu on the menu page itself.
                    if self.context_menu.as_ref().map(|c| c.menu_browser_id)
                        == Some(page.browser.identifier())
                    {
                        return;
                    }
                    let browser_id = page.browser.identifier();
                    let page_url = page.url();
                    let ephemeral = page.ephemeral;
                    if ephemeral {
                        // Utility pages have no autofill bridge — page-level menu only.
                        pending_actions::open_context_menu(
                            &mut self.session,
                            state,
                            &mut self.context_menu,
                            self.cursor_window,
                            crate::pages::context_menu::MenuContext {
                                on_canvas: false,
                                page_url: Some(page_url),
                                target_browser_id: Some(browser_id),
                                ..Default::default()
                            },
                        );
                    } else {
                        let local_x = (cursor_world.0 - page.rect.x) / scale as f32;
                        let local_y = (cursor_world.1 - page.rect.y) / scale as f32;
                        self.pending_context_hit =
                            Some(crate::pending_actions::ContextHitRequest {
                                browser_id,
                                screen_pos: self.cursor_window,
                                at: std::time::Instant::now(),
                            });
                        if let Some(frame) = page.browser.main_frame() {
                            let js = format!(
                                "window.__spatialContextHitAt && window.__spatialContextHitAt({local_x},{local_y});"
                            );
                            frame.execute_java_script(
                                Some(&js.as_str().into()),
                                Some(&"".into()),
                                0,
                            );
                        }
                    }
                }
            }
            return;
        }

        let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
        let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1) else {
            return;
        };
        let i = if element_state == ElementState::Pressed {
            self.session.bring_to_front(i)
        } else {
            i
        };
        let page = &self.session.pages()[i];
        let host = page.browser.host();
        self.mouse.button(element_state, button, host.as_ref());
    }

    /// `WindowEvent::MouseWheel`.
    pub(super) fn on_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        if self.modifiers.control_key() {
            let scroll_y = match delta {
                winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
            };
            let factor = 1.1f32.powf(scroll_y);
            self.session.zoom_viewport_at(self.cursor_window, factor);
            return;
        }

        let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
        if let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1) {
            let host = self.session.pages()[i].browser.host();
            self.mouse.wheel(delta, host.as_ref());
        }
    }

    /// `WindowEvent::Touch`.
    pub(super) fn on_touch(&mut self, touch: Touch) {
        let Some(state) = &mut self.state else {
            return;
        };
        let scale = state.window.scale_factor() as f32;
        let window_pos = (touch.location.x as f32, touch.location.y as f32);
        // Keep cursor_window in sync so pinch-from-trackpad /
        // subsequent mouse wheel zoom still has a sensible pivot
        // after a touch interaction.
        self.cursor_window = window_pos;
        let viewport = self.session.viewport();
        let world = viewport.screen_to_world(window_pos);
        let hit = hit_test(self.session.pages(), world.0, world.1).map(|i| {
            let page = &self.session.pages()[i];
            TouchHit {
                browser_id: page.browser.identifier(),
                local: (
                    (world.0 - page.rect.x) / scale,
                    (world.1 - page.rect.y) / scale,
                ),
            }
        });
        let cmds = self
            .touch
            .handle(&touch, hit, viewport.offset, viewport.zoom);
        for cmd in cmds {
            match cmd {
                TouchCmd::Send {
                    browser_id,
                    id,
                    local,
                    type_,
                    pressure,
                } => {
                    if let Some(page) = self
                        .session
                        .pages()
                        .iter()
                        .find(|p| p.browser.identifier() == browser_id)
                    {
                        if let Some(host) = page.browser.host() {
                            send_touch_to_host(&host, id, local, type_, pressure);
                        }
                    }
                }
                TouchCmd::PanViewport { offset } => {
                    self.session.pan_viewport_to(offset);
                }
                TouchCmd::SetViewport { offset, zoom } => {
                    self.session.set_viewport(Viewport { offset, zoom });
                }
            }
        }
    }
}
