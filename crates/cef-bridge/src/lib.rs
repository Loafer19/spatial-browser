//! FFI bridge to CEF (Chromium Embedded Framework), off-screen rendering
//! with shared GPU textures. Each `OsrRenderHandler` owns its own texture
//! slot (returned from `OsrRenderHandler::new`), so multiple simultaneous
//! browser instances — multiple pages on the spatial canvas — each paint
//! into their own texture rather than a shared global.
//!
//! One file per CEF client-handler interface this bridge implements —
//! `app`, `render`, `display`, `download`, `life_span`, `load` each wrap
//! exactly one `cef::ImplXxxHandler`. `navigation` is the one
//! exception: it groups every custom-scheme (`bookmark://`,
//! `omnibox://`, `switcher://`, `download://`, `history://`)
//! interception together, since all five are the same `RequestHandler`
//! dispatching on a URL prefix — one concern, not five. `client` ties
//! all the handlers into the one `cef::Client` CEF hands to each
//! spawned browser. Every module is private; everything `pub` in them
//! is re-exported flat here so callers keep using `cef_bridge::Thing`
//! rather than `cef_bridge::whichever_module::Thing`.
//!
//! Deliberately no request-context handler: every page passes `None`
//! for its request context (browser.rs), so they all share CEF's one
//! global context (persisted via main.rs's `cache_path`) instead of
//! each getting its own fresh, isolated, in-memory-only one — the
//! previous per-page context meant no page shared a login with any
//! other, and every restart (including scripts/run.sh's auto-restart
//! on the known SPA-navigation crash) silently logged everything out.

mod app;
mod blocklist;
mod clean_urls;
mod client;
mod display;
mod download;
mod life_span;
mod load;
mod navigation;
mod render;

pub use app::*;
pub use blocklist::{set_custom_hosts, set_enabled};
pub use clean_urls::set_enabled as set_clean_urls_enabled;
pub use client::*;
pub use display::*;
pub use download::*;
pub use life_span::*;
pub use load::*;
pub use navigation::*;
pub use render::*;
