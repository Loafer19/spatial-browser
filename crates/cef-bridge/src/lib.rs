//! CEF OSR bridge: one module per handler; `navigation` groups custom-scheme
//! intercepts. Pages pass `None` request context so all share one persistent
//! global (see main.rs `cache_path`) — per-page contexts broke shared logins.

mod app;
mod blocklist;
pub mod clipboard;
mod filter_engine;
mod find;
mod clean_urls;
mod client;
mod display;
mod download;
mod life_span;
mod load;
mod message_pump;
mod navigation;
mod paint_signal;
mod render;
pub mod spatial;

pub use app::*;
pub use blocklist::{set_custom_hosts, set_enabled, set_peter_lowe_enabled};
pub use filter_engine::{
    cosmetic_hide_css, cosmetic_inject_js, rebuild as rebuild_filter_engine, wait_idle as wait_filter_engine, scriptlet_inject_js,
    scriptlet_js, set_cosmetic_enabled, set_scriptlets_enabled, FilterEngineConfig,
};
pub use clean_urls::set_enabled as set_clean_urls_enabled;
pub use client::*;
pub use display::*;
pub use find::*;
pub use download::*;
pub use life_span::*;
pub use load::*;
pub use message_pump::{
    next_message_pump_work, set_message_pump_waker, take_message_pump_work, MAX_PUMP_DELAY,
};
pub use navigation::*;
pub use paint_signal::{note_paint, paint_count};
pub use render::*;
