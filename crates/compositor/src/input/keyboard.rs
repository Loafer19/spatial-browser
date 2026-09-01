// winit keys → CEF send_key_event. Chromium wants Windows VKEYs;
// punctuation OEM_* assumes US/QWERTY. RAWKEYDOWN/KEYUP + CHAR from KeyEvent::text.

use cef::{BrowserHost, ImplBrowserHost, KeyEvent as CefKeyEvent, KeyEventType};
use winit::event::ElementState;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

// cef_event_flags_t bits relevant here (see include/internal/cef_types.h).
const EVENTFLAG_SHIFT_DOWN: u32 = 1 << 1;
const EVENTFLAG_CONTROL_DOWN: u32 = 1 << 2;
const EVENTFLAG_ALT_DOWN: u32 = 1 << 3;
const EVENTFLAG_COMMAND_DOWN: u32 = 1 << 7;

#[derive(Default)]
pub struct KeyboardInput {
    modifiers: u32,
}

impl KeyboardInput {
    pub fn modifiers_changed(&mut self, state: ModifiersState) {
        self.modifiers = 0;
        if state.shift_key() {
            self.modifiers |= EVENTFLAG_SHIFT_DOWN;
        }
        if state.control_key() {
            self.modifiers |= EVENTFLAG_CONTROL_DOWN;
        }
        if state.alt_key() {
            self.modifiers |= EVENTFLAG_ALT_DOWN;
        }
        if state.super_key() {
            self.modifiers |= EVENTFLAG_COMMAND_DOWN;
        }
    }

    pub fn key_event(&self, event: &winit::event::KeyEvent, host: Option<&BrowserHost>) {
        let Some(host) = host else { return };
        let Some(vkey) = physical_key_to_vkey(event.physical_key) else {
            return;
        };

        let type_ = match event.state {
            ElementState::Pressed => KeyEventType::RAWKEYDOWN,
            ElementState::Released => KeyEventType::KEYUP,
        };
        host.send_key_event(Some(&CefKeyEvent {
            type_,
            modifiers: self.modifiers,
            windows_key_code: vkey,
            native_key_code: vkey,
            ..Default::default()
        }));

        // Text this key press produces (layout- and modifier-aware; empty
        // for pure control keys like arrows or a bare Shift) goes as
        // separate CHAR events, one per UTF-16 code unit.
        if event.state == ElementState::Pressed {
            if let Some(text) = &event.text {
                for ch in text.chars() {
                    let mut units = [0u16; 2];
                    for &mut unit in ch.encode_utf16(&mut units) {
                        host.send_key_event(Some(&CefKeyEvent {
                            type_: KeyEventType::CHAR,
                            modifiers: self.modifiers,
                            windows_key_code: unit as i32,
                            character: unit,
                            unmodified_character: unit,
                            ..Default::default()
                        }));
                    }
                }
            }
        }
    }
}

/// PhysicalKey → Windows VKEY; uncovered keys (media/IME) dropped.
fn physical_key_to_vkey(key: PhysicalKey) -> Option<i32> {
    let PhysicalKey::Code(code) = key else {
        return None;
    };
    Some(match code {
        KeyCode::KeyA => 0x41,
        KeyCode::KeyB => 0x42,
        KeyCode::KeyC => 0x43,
        KeyCode::KeyD => 0x44,
        KeyCode::KeyE => 0x45,
        KeyCode::KeyF => 0x46,
        KeyCode::KeyG => 0x47,
        KeyCode::KeyH => 0x48,
        KeyCode::KeyI => 0x49,
        KeyCode::KeyJ => 0x4A,
        KeyCode::KeyK => 0x4B,
        KeyCode::KeyL => 0x4C,
        KeyCode::KeyM => 0x4D,
        KeyCode::KeyN => 0x4E,
        KeyCode::KeyO => 0x4F,
        KeyCode::KeyP => 0x50,
        KeyCode::KeyQ => 0x51,
        KeyCode::KeyR => 0x52,
        KeyCode::KeyS => 0x53,
        KeyCode::KeyT => 0x54,
        KeyCode::KeyU => 0x55,
        KeyCode::KeyV => 0x56,
        KeyCode::KeyW => 0x57,
        KeyCode::KeyX => 0x58,
        KeyCode::KeyY => 0x59,
        KeyCode::KeyZ => 0x5A,

        KeyCode::Digit0 => 0x30,
        KeyCode::Digit1 => 0x31,
        KeyCode::Digit2 => 0x32,
        KeyCode::Digit3 => 0x33,
        KeyCode::Digit4 => 0x34,
        KeyCode::Digit5 => 0x35,
        KeyCode::Digit6 => 0x36,
        KeyCode::Digit7 => 0x37,
        KeyCode::Digit8 => 0x38,
        KeyCode::Digit9 => 0x39,

        KeyCode::Backspace => 0x08,
        KeyCode::Tab => 0x09,
        KeyCode::Enter | KeyCode::NumpadEnter => 0x0D,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => 0x10,
        KeyCode::ControlLeft | KeyCode::ControlRight => 0x11,
        KeyCode::AltLeft | KeyCode::AltRight => 0x12,
        KeyCode::Escape => 0x1B,
        KeyCode::Space => 0x20,
        KeyCode::PageUp => 0x21,
        KeyCode::PageDown => 0x22,
        KeyCode::End => 0x23,
        KeyCode::Home => 0x24,
        KeyCode::ArrowLeft => 0x25,
        KeyCode::ArrowUp => 0x26,
        KeyCode::ArrowRight => 0x27,
        KeyCode::ArrowDown => 0x28,
        KeyCode::Delete => 0x2E,
        KeyCode::SuperLeft => 0x5B,
        KeyCode::SuperRight => 0x5C,

        KeyCode::F1 => 0x70,
        KeyCode::F2 => 0x71,
        KeyCode::F3 => 0x72,
        KeyCode::F4 => 0x73,
        KeyCode::F5 => 0x74,
        KeyCode::F6 => 0x75,
        KeyCode::F7 => 0x76,
        KeyCode::F8 => 0x77,
        KeyCode::F9 => 0x78,
        KeyCode::F10 => 0x79,
        KeyCode::F11 => 0x7A,
        KeyCode::F12 => 0x7B,

        // Standard-US-layout punctuation (VKEY_OEM_*).
        KeyCode::Semicolon => 0xBA,
        KeyCode::Equal => 0xBB,
        KeyCode::Comma => 0xBC,
        KeyCode::Minus => 0xBD,
        KeyCode::Period => 0xBE,
        KeyCode::Slash => 0xBF,
        KeyCode::Backquote => 0xC0,
        KeyCode::BracketLeft => 0xDB,
        KeyCode::Backslash => 0xDC,
        KeyCode::BracketRight => 0xDD,
        KeyCode::Quote => 0xDE,

        _ => return None,
    })
}
