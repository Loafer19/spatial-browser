// Native file pick on a background thread — XDG portal times out (~2s) if
// the compositor event loop is blocked on the dialog.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PickKind {
    Bookmarks,
    PasswordsCsv,
}

pub struct PickOptions {
    pub title: String,
    pub start_dir: Option<PathBuf>,
    pub file_name: Option<String>,
    /// `(label, extensions)` — e.g. `("CSV", vec!["csv"])`. Empty = no filter.
    pub filters: Vec<(String, Vec<String>)>,
}

struct AsyncFilePick {
    browser_id: i32,
    rx: Receiver<Option<PathBuf>>,
}

static PENDING: LazyLock<Mutex<HashMap<PickKind, AsyncFilePick>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Spawn the system file dialog off-thread; result arrives via [`poll`].
pub fn start(kind: PickKind, browser_id: i32, opts: PickOptions) {
    let (tx, rx) = mpsc::channel();
    if let Ok(mut map) = PENDING.lock() {
        map.insert(kind, AsyncFilePick { browser_id, rx });
    }
    let thread_name = match kind {
        PickKind::Bookmarks => "bookmark-file-dialog",
        PickKind::PasswordsCsv => "csv-file-dialog",
    };
    let _ = std::thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title(&opts.title);
            for (label, exts) in &opts.filters {
                let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
                dialog = dialog.add_filter(label, &refs);
            }
            if let Some(dir) = opts.start_dir.filter(|p| p.is_dir()) {
                dialog = dialog.set_directory(dir);
            }
            if let Some(name) = opts.file_name {
                dialog = dialog.set_file_name(name);
            }
            let _ = tx.send(dialog.pick_file());
        });
}

/// Non-blocking: `Some((browser_id, chosen_path_or_none))` when the dialog closes.
pub fn poll(kind: PickKind) -> Option<(i32, Option<PathBuf>)> {
    let mut map = PENDING.lock().ok()?;
    let pending = map.get(&kind)?;
    match pending.rx.try_recv() {
        Ok(path) => {
            let browser_id = pending.browser_id;
            map.remove(&kind);
            Some((browser_id, path))
        }
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => {
            map.remove(&kind);
            None
        }
    }
}
