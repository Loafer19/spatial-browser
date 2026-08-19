// Persisted canvas snapshots — named save points a user can restore
// the whole canvas (pages, viewport, theme) from, distinct from the
// live session.json this project already debounce-autosaves.
// Deliberately a save-point model, not a "switch between live
// desktops" one: loading a workspace replaces every currently open
// page with the saved ones, the way loading a saved game replaces
// unsaved progress, rather than the compositor tracking an "active
// workspace" and auto-saving back into it on every switch. Bookmarks/
// downloads/history stay global — this is only the canvas layout
// itself.

use crate::output::Rect;
use crate::viewport::Viewport;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspacePage {
    pub url: String,
    pub rect: Rect,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub viewport: Viewport,
    pub theme: String,
    pub pages: Vec<WorkspacePage>,
}

#[derive(Default, Serialize, Deserialize)]
struct WorkspacesFile {
    entries: Vec<Workspace>,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/workspaces.json")
}

/// Returns an empty list if there's no file yet or it fails to parse.
pub fn load() -> Vec<Workspace> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<WorkspacesFile>(&bytes).ok())
        .map(|f| f.entries)
        .unwrap_or_default()
}

/// Workspace edits (save/rename/delete) are rare, deliberate actions —
/// saved immediately, no debounce.
pub fn save(workspaces: &[Workspace]) {
    let data = WorkspacesFile {
        entries: workspaces.to_vec(),
    };
    let path = path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("workspaces save: couldn't create {parent:?}: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(&data) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("workspaces save: couldn't write {path:?}: {e}");
            }
        }
        Err(e) => log::warn!("workspaces save: serialize failed: {e}"),
    }
}
