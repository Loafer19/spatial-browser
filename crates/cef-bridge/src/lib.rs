//! FFI bridge to CEF (Chromium Embedded Framework).
//!
//! Roadmap: initialize CEF, spawn one CefBrowser per page in off-screen
//! render mode with shared GPU textures (`OnAcceleratedPaint`), hand the
//! texture handles to the compositor crate each frame.

pub fn placeholder() {}
