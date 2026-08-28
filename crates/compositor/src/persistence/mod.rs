// Everything that reads/writes a JSON file under
// ~/.config/spatial-browser/: this module is the canvas session itself
// (save/load below); bookmarks.rs, history.rs, typed_history.rs,
// downloads.rs, and workspaces.rs are their own separate files/concerns
// (bookmarks change rarely and deliberately, history.rs is a log of
// actual page visits, typed_history.rs is a flat list of typed omnibox
// input — a deliberately different name so the two don't get confused —
// downloads is a log of completed CEF downloads, workspaces is live slots
// named canvas snapshots distinct from the live session below,
// settings.rs is the one user-editable preferences object) grouped
// here because they share the same shape of problem, not because they
// share data.
//
// Canvas session: active theme, viewport pan/zoom, and each page's
// URL/rect (z-order = list order). One JSON file — the whole point of a
// spatial canvas is one session, not per-window profiles (see
// single_instance.rs) — written debounced on change and once more on
// clean exit (see app.rs).

pub mod bookmarks;
pub mod downloads;
pub mod history;
pub mod settings;
pub mod typed_history;
pub mod vault;
pub mod vault_csv;
pub mod workspaces;

use crate::browser;
use crate::output::{GpuState, Rect, THEMES};
use crate::session::Session;
use crate::viewport::Viewport;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use winit::window::Window;

#[derive(Serialize, Deserialize)]
struct SessionFile {
    theme: String,
    viewport: Viewport,
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

pub fn save(session: &Session) {
    let data = SessionFile {
        theme: session.theme().name.to_string(),
        viewport: session.viewport(),
        // Skips ephemeral pages (F1 help, bookmarks list): they're
        // regenerated fresh from current data whenever reopened, so
        // persisting one would just freeze a stale snapshot that reopens
        // on every future launch instead of real content.
        pages: session
            .pages()
            .iter()
            .filter(|p| !p.ephemeral)
            .map(|p| PageFile {
                url: p.url(),
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
        .map(|p| browser::spawn(gpu, window, &p.url, p.rect, false))
        .collect();
    Some(Session::new(pages, data.viewport, theme))
}
