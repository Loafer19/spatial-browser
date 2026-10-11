// JSON under ~/.config/spatial-browser/: session (theme/viewport/page rects)
// plus sibling modules for bookmarks, history, downloads, workspaces, settings.

pub mod bookmarks;
pub mod bookmarks_chrome;
pub mod downloads;
pub mod favicons;
pub mod history;
pub mod settings;
pub mod store;
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

/// `~/.config/spatial-browser/<name>`: every persisted file lives here.
/// `$HOME` is part of the runtime contract (CEF's own cache dir is under
/// it too), so a missing `HOME` is a startup misconfiguration, not a
/// recoverable error.
pub fn config_path(name: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .expect("HOME must be set: profile data lives under ~/.config/spatial-browser");
    PathBuf::from(home)
        .join(".config/spatial-browser")
        .join(name)
}

fn path() -> PathBuf {
    crate::persistence::config_path("session.json")
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

    store::save_json_bg(&path(), "session", &data, store::Backup::Keep);
}

/// Load session and spawn pages, or `None` → caller uses default layout.
pub fn load(gpu: &GpuState, window: &Window) -> Option<Session> {
    let data: SessionFile = store::load_json(&path(), "session")?;
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
