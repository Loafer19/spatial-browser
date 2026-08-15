// Saves/restores the canvas across restarts: active theme, camera
// pan/zoom, and each page's URL/rect (z-order = list order). One JSON
// file — the whole point of a spatial canvas is one session, not
// per-window profiles (see single_instance.rs) — written debounced on
// change and once more on clean exit (see app.rs).

use crate::browser::{self, Page};
use crate::camera::Camera;
use crate::output::{GpuState, Rect, THEMES};
use crate::session::Session;
use cef::{ImplBrowser, ImplFrame};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use winit::window::Window;

#[derive(Serialize, Deserialize)]
struct SessionFile {
    theme: String,
    camera: Camera,
    pages: Vec<PageFile>,
}

#[derive(Serialize, Deserialize)]
struct PageFile {
    url: String,
    rect: Rect,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/session.json")
}

/// Reads a page's *current* URL straight from CEF rather than tracking
/// it ourselves, so in-page navigation (a clicked link, back/forward)
/// is captured too, not just the URL it was spawned with.
fn current_url(page: &Page) -> String {
    page.browser
        .main_frame()
        .map(|frame| cef::CefString::from(&frame.url()).to_string())
        .unwrap_or_default()
}

pub fn save(session: &Session) {
    let data = SessionFile {
        theme: session.theme().name.to_string(),
        camera: session.camera(),
        pages: session
            .pages()
            .iter()
            .map(|p| PageFile {
                url: current_url(p),
                rect: p.rect,
            })
            .collect(),
    };

    let path = path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("session save: couldn't create {parent:?}: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(&data) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("session save: couldn't write {path:?}: {e}");
            }
        }
        Err(e) => log::warn!("session save: serialize failed: {e}"),
    }
}

/// Loads the saved session and spawns its pages, or `None` if there's no
/// save file yet or it fails to parse — the caller falls back to a
/// default layout either way.
pub fn load(gpu: &GpuState, window: &Window) -> Option<Session> {
    let bytes = std::fs::read(path()).ok()?;
    let data: SessionFile = serde_json::from_slice(&bytes).ok()?;
    let theme = THEMES
        .iter()
        .find(|t| t.name == data.theme)
        .copied()
        .unwrap_or(THEMES[0]);
    let pages = data
        .pages
        .into_iter()
        .map(|p| browser::spawn(gpu, window, &p.url, p.rect))
        .collect();
    Some(Session::new(pages, data.camera, theme))
}
