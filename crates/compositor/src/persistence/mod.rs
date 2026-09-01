// JSON under ~/.config/spatial-browser/: session (theme/viewport/page rects)
// plus sibling modules for bookmarks, history, downloads, workspaces, settings.

pub mod bookmarks;
pub mod bookmarks_chrome;
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
    // Persists: theme, pre-zoom viewport, non-ephemeral page URL+layout_rect.
    // Not persisted: ephemeral overlays, reader_mode, load_*/progress, zoomed live rect.
    let data = SessionFile {
        theme: session.theme().name.to_string(),
        viewport: session.layout_viewport(),
        pages: session
            .pages()
            .iter()
            .filter(|p| !p.ephemeral)
            .map(|p| PageFile {
                url: p.url(),
                rect: p.layout_rect(),
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

/// Load session and spawn pages, or `None` → caller uses default layout.
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
