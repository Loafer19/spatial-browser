// Single-instance: one canvas/session. Second launch focuses the existing window
// (avoids racing session.json / CEF cache_path).

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;

fn socket_path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/instance.sock")
}

/// If another instance is already running, asks it to focus its window
/// and returns `true` — the caller should exit immediately, before
/// paying for CEF's `initialize()`. Otherwise starts listening for
/// future launches and returns `false`.
pub fn acquire_or_notify() -> bool {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if UnixStream::connect(&path).is_ok() {
        return true;
    }

    // No live listener on the other end — any socket file here is stale
    // (e.g. left behind by an unclean shutdown); clear it before binding.
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else {
        // Couldn't bind (permissions, read-only fs, ...) — not worth
        // blocking startup over; just run unlinked from any other
        // instance rather than fail entirely.
        return false;
    };

    let pid = std::process::id();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            drop(stream);
            let _ = Command::new("hyprctl")
                .args(["dispatch", "focuswindow", &format!("pid:{pid}")])
                .spawn();
        }
    });

    false
}
