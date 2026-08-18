//! FFI bridge to CEF (Chromium Embedded Framework), off-screen rendering
//! with shared GPU textures. Each `OsrRenderHandler` owns its own texture
//! slot (returned from `OsrRenderHandler::new`), so multiple simultaneous
//! browser instances — multiple pages on the spatial canvas — each paint
//! into their own texture rather than a shared global.
//!
//! One file per CEF client-handler interface this bridge implements —
//! `app`, `render`, `display`, `download`, `life_span`, `load` each wrap
//! exactly one `cef::ImplXxxHandler`, plus `request_context` for the
//! (currently empty) per-browser request context handler. `navigation`
//! is the one exception: it groups every custom-scheme (`bookmark://`,
//! `omnibox://`, `switcher://`, `download://`, `history://`)
//! interception together, since all five are the same `RequestHandler`
//! dispatching on a URL prefix — one concern, not five. `client` ties
//! all the handlers into the one `cef::Client` CEF hands to each
//! spawned browser. Every module is private; everything `pub` in them
//! is re-exported flat here so callers keep using `cef_bridge::Thing`
//! rather than `cef_bridge::whichever_module::Thing`.

mod app;
mod client;
mod display;
mod download;
mod life_span;
mod load;
mod navigation;
mod render;
mod request_context;

pub use app::*;
pub use client::*;
pub use display::*;
pub use download::*;
pub use life_span::*;
pub use load::*;
pub use navigation::*;
pub use render::*;
pub use request_context::*;
