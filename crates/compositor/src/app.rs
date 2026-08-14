// Routes winit window events to the right place: mouse/keyboard input to
// the CEF browser, resize/redraw to the GpuState. This is the glue layer
// between "winit's ApplicationHandler" and everything else in this crate.

use crate::browser::{self, BrowserState};
use crate::input::{KeyboardInput, MouseInput};
use crate::output::{FrameOutcome, GpuState};
use cef::{ImplBrowser, ImplBrowserHost};
use cef_bridge::CURSOR;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{WindowAttributes, WindowId},
};

pub const HOME_PAGE: &str = "https://example.com";

#[derive(Default)]
pub struct App {
    state: Option<GpuState>,
    browser: Option<BrowserState>,
    mouse: MouseInput,
    keyboard: KeyboardInput,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(WindowAttributes::default().with_title("spatial-browser"))
                .expect("failed to create window"),
        );
        let state = pollster::block_on(GpuState::new(window.clone()));
        self.browser = Some(browser::spawn(&state, &window, HOME_PAGE));
        self.state = Some(state);
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        if state.window.id() != id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);
                if let Some(browser) = &self.browser {
                    *browser.size.borrow_mut() = size.to_logical(state.window.scale_factor());
                    if let Some(host) = browser.browser.host() {
                        host.was_resized();
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = state.window.scale_factor();
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse
                    .cursor_moved((position.x, position.y), scale, host.as_ref());
            }
            WindowEvent::CursorLeft { .. } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse.cursor_left(host.as_ref());
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse.button(element_state, button, host.as_ref());
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse.wheel(delta, host.as_ref());
            }
            WindowEvent::Focused(focused) => {
                if let Some(host) = self.browser.as_ref().and_then(|b| b.browser.host()) {
                    host.set_focus(focused as _);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.keyboard.modifiers_changed(modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.keyboard.key_event(&event, host.as_ref());
            }
            WindowEvent::RedrawRequested => {
                if let Some(host) = self.browser.as_ref().and_then(|b| b.browser.host()) {
                    host.send_external_begin_frame();
                }
                if let Some(icon) = CURSOR.with_borrow_mut(|cursor| cursor.take()) {
                    state.window.set_cursor(icon);
                }
                match state.render() {
                    FrameOutcome::Rendered | FrameOutcome::Skip => {}
                    FrameOutcome::Reconfigure => {
                        let size = state.window.inner_size();
                        state.resize(size.width, size.height);
                    }
                    FrameOutcome::Fatal => {
                        log::error!("fatal surface validation error, exiting");
                        event_loop.exit();
                    }
                }
                state.window.request_redraw();
            }
            _ => {}
        }
    }
}
