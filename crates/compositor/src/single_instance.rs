// Single-instance: one canvas/session. A second launch hands its URL
// arguments to the running instance and focuses its window (avoids racing
// session.json / CEF cache_path).

use crate::external::is_web_url;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

/// URLs to open as new pages: from our own argv at startup, then from later
/// launches. Drained by the frame loop (`take_incoming`).
static INCOMING: Mutex<Vec<String>> = Mutex::new(Vec::new());
const MAX_URLS: usize = 20;
const MAX_MESSAGE: u64 = 64 * 1024;

fn socket_path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/instance.sock")
}

/// http(s) URLs among the launch arguments (CEF switches are skipped).
pub fn launch_urls() -> Vec<String> {
    // args_os: a non-UTF-8 argument (e.g. a Latin-1 path from a file
    // manager) must not panic before CEF is up; it's just not a URL.
    web_urls(
        std::env::args_os()
            .skip(1)
            .filter_map(|a| a.into_string().ok()),
    )
}

fn web_urls(items: impl IntoIterator<Item = String>) -> Vec<String> {
    items
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| is_web_url(s))
        .take(MAX_URLS)
        .collect()
}

pub fn take_incoming() -> Vec<String> {
    INCOMING
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

fn queue(urls: Vec<String>) {
    if let Ok(mut q) = INCOMING.lock() {
        q.extend(urls);
        q.truncate(MAX_URLS);
    }
}

/// If another instance is already running, sends it `urls`, asks it to
/// focus, and returns `true` — the caller should exit before paying for
/// CEF's `initialize()`. Otherwise queues `urls` for this instance, starts
/// listening for future launches and returns `false`.
pub fn acquire_or_notify(urls: &[String]) -> bool {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(mut stream) = UnixStream::connect(&path) {
        let message: String = urls.iter().map(|u| format!("{u}\n")).collect();
        let _ = stream.write_all(message.as_bytes());
        return true;
    }

    queue(urls.to_vec());

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
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut text = String::new();
            let _ = stream.take(MAX_MESSAGE).read_to_string(&mut text);
            queue(web_urls(text.lines().map(str::to_string)));
            let _ = Command::new("hyprctl")
                .args(["dispatch", "focuswindow", &format!("pid:{pid}")])
                .spawn();
        }
    });

    false
}

#[cfg(test)]
mod tests {
    use super::web_urls;

    #[test]
    fn keeps_only_web_urls() {
        let args = [
            "--type=renderer",
            "https://a.test",
            " http://b.test ",
            "file:///x",
            "foo",
        ];
        assert_eq!(
            web_urls(args.iter().map(|s| s.to_string())),
            vec!["https://a.test".to_string(), "http://b.test".to_string()]
        );
    }
}
