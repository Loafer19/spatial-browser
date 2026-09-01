// winit mouse → CEF host events. Convert physical px → CEF DIP via scale_factor.

use cef::{BrowserHost, ImplBrowserHost, MouseButtonType, MouseEvent};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

// cef_event_flags_t bits relevant here (see include/internal/cef_types.h);
// the `cef` crate's `MouseEvent::modifiers` is a plain `u32` of these.
const EVENTFLAG_LEFT_MOUSE_BUTTON: u32 = 1 << 4;
const EVENTFLAG_MIDDLE_MOUSE_BUTTON: u32 = 1 << 5;
const EVENTFLAG_RIGHT_MOUSE_BUTTON: u32 = 1 << 6;

#[derive(Default)]
pub struct MouseInput {
    // Logical (DIP) cursor position, in CEF's coordinate space — kept
    // around because MouseInput/MouseWheel winit events don't carry a
    // position of their own.
    cursor_logical: (i32, i32),
    // Bitmask of EVENTFLAG_*_MOUSE_BUTTON for buttons currently held. CEF
    // wants the held-button mask on move events too (for drag detection),
    // not just on the click event itself.
    buttons_down: u32,
}

impl MouseInput {
    fn event(&self) -> MouseEvent {
        MouseEvent {
            x: self.cursor_logical.0,
            y: self.cursor_logical.1,
            modifiers: self.buttons_down,
        }
    }

    pub fn cursor_moved(
        &mut self,
        physical: (f64, f64),
        scale_factor: f64,
        host: Option<&BrowserHost>,
    ) {
        self.cursor_logical = (
            (physical.0 / scale_factor) as i32,
            (physical.1 / scale_factor) as i32,
        );
        if let Some(host) = host {
            host.send_mouse_move_event(Some(&self.event()), 0);
        }
    }

    pub fn cursor_left(&mut self, host: Option<&BrowserHost>) {
        if let Some(host) = host {
            host.send_mouse_move_event(Some(&self.event()), 1);
        }
    }

    pub fn button(&mut self, state: ElementState, button: MouseButton, host: Option<&BrowserHost>) {
        let (cef_button, flag) = match button {
            MouseButton::Left => (MouseButtonType::LEFT, EVENTFLAG_LEFT_MOUSE_BUTTON),
            MouseButton::Middle => (MouseButtonType::MIDDLE, EVENTFLAG_MIDDLE_MOUSE_BUTTON),
            MouseButton::Right => (MouseButtonType::RIGHT, EVENTFLAG_RIGHT_MOUSE_BUTTON),
            _ => return,
        };
        let mouse_up = state == ElementState::Released;
        if mouse_up {
            self.buttons_down &= !flag;
        } else {
            self.buttons_down |= flag;
        }
        if let Some(host) = host {
            host.send_mouse_click_event(Some(&self.event()), cef_button, mouse_up as _, 1);
        }
    }

    pub fn wheel(&self, delta: MouseScrollDelta, host: Option<&BrowserHost>) {
        // Line-to-pixel scale is arbitrary (CEF/Chromium doesn't define a
        // canonical one for injected events) — 120px/line matches other
        // browsers' scroll-by-line amount for a real mouse wheel.
        //
        // Trackpad smooth-scroll (PixelDelta) needs a much bigger factor:
        // measured raw deltas on this machine's touchpad are ~1-3 physical
        // px/event, so anything close to 1:1 passthrough is imperceptible
        // — other apps are clearly amplifying it well beyond that to feel
        // natural.
        let (delta_x, delta_y) = match delta {
            MouseScrollDelta::LineDelta(x, y) => ((x * 120.0) as i32, (y * 120.0) as i32),
            MouseScrollDelta::PixelDelta(pos) => ((pos.x * 8.0) as i32, (pos.y * 8.0) as i32),
        };
        if let Some(host) = host {
            host.send_mouse_wheel_event(Some(&self.event()), delta_x, delta_y);
        }
    }
}
