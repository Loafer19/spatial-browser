// OSR has no OS clipboard surface. Copy: inject COPY_BRIDGE_SCRIPT on each
// load → clipboard://copy → cef_bridge::clipboard (gated on user intent) →
// wl-copy. Paste: hotkeys intercept Ctrl+V → start_paste runs wl-paste off
// the UI thread → deliver_paste (per frame) runs execCommand('insertText')
// (fires real input events; works in contenteditable).

use crate::session::Session;
use cef::{ImplBrowser, ImplFrame};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Mutex;
use std::time::Duration;

pub const COPY_BRIDGE_SCRIPT: &str = r#"
(function() {
  if (window.__spatialClipboardBridge__) return;
  window.__spatialClipboardBridge__ = true;
  document.addEventListener('copy', function() {
    var text = window.getSelection().toString();
    if (text) {
      location = 'clipboard://copy?text=' + encodeURIComponent(text);
    }
  }, true);
})();
"#;

/// JS single-quoted literal for execute_java_script (no HTML escape — raw JS).
pub fn js_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

/// A clipboard owner that never answers would otherwise hang wl-paste forever.
const PASTE_TIMEOUT: Duration = Duration::from_secs(2);

struct PendingPaste {
    browser_id: i32,
    rx: Receiver<Option<String>>,
}

// Latest Ctrl+V wins; an older in-flight read is dropped (its send fails).
static PENDING_PASTE: Mutex<Option<PendingPaste>> = Mutex::new(None);

/// `-n`: no trailing newline. `-t text`: wl-paste picks a textual offer, so
/// an image or file selection is never inserted as binary garbage.
fn wl_paste_args() -> [&'static str; 3] {
    ["--no-newline", "--type", "text"]
}

fn kill_and_reap(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Run wl-paste, killing it after `timeout`. `None` = empty, non-text, or
/// failed clipboard (wl-paste exits non-zero when nothing matches).
fn read_clipboard(timeout: Duration) -> Option<String> {
    let mut child = Command::new("wl-paste")
        .args(wl_paste_args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| log::warn!("wl-paste failed (paste unavailable): {e}"))
        .ok()?;
    let Some(mut stdout) = child.stdout.take() else {
        kill_and_reap(&mut child);
        return None;
    };
    let (tx, rx) = mpsc::channel();
    // Reader thread: exits on EOF, including after a kill below.
    let reader = std::thread::Builder::new()
        .name("wl-paste-read".into())
        .spawn(move || {
            let mut buf = Vec::new();
            let _ = tx.send(stdout.read_to_end(&mut buf).map(|_| buf));
        });
    if reader.is_err() {
        kill_and_reap(&mut child);
        return None;
    }
    let bytes = match rx.recv_timeout(timeout) {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            log::warn!("wl-paste read failed: {e}");
            kill_and_reap(&mut child);
            return None;
        }
        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
            log::warn!("wl-paste timed out after {timeout:?}; clipboard owner not responding");
            kill_and_reap(&mut child);
            return None;
        }
    };
    // stdout hit EOF, so wl-paste is exiting; this wait is short.
    let status = child.wait().ok()?;
    if !status.success() || bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Ctrl+V: read the clipboard off the UI thread; [`deliver_paste`] inserts it.
pub fn start_paste(browser_id: i32) {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("wl-paste".into())
        .spawn(move || {
            let _ = tx.send(read_clipboard(PASTE_TIMEOUT));
        });
    if let Err(e) = spawned {
        log::warn!("couldn't start wl-paste thread: {e}");
        return;
    }
    if let Ok(mut slot) = PENDING_PASTE.lock() {
        *slot = Some(PendingPaste { browser_id, rx });
    }
}

/// Non-blocking: `Some((browser_id, text))` once a read finishes with text.
fn poll_paste(slot: &Mutex<Option<PendingPaste>>) -> Option<(i32, String)> {
    let mut slot = slot.lock().ok()?;
    let pending = slot.as_ref()?;
    match pending.rx.try_recv() {
        Ok(text) => {
            let browser_id = pending.browser_id;
            *slot = None;
            text.map(|t| (browser_id, t))
        }
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => {
            *slot = None;
            None
        }
    }
}

fn insert_text_script(text: &str) -> String {
    format!(
        "document.execCommand('insertText', false, {});",
        js_literal(text)
    )
}

/// Per frame: insert finished Ctrl+V text into the requesting page's focused
/// frame (editors inside iframes), else its main frame.
pub fn deliver_paste(session: &Session) {
    let Some((browser_id, text)) = poll_paste(&PENDING_PASTE) else {
        return;
    };
    let Some(page) = session
        .pages()
        .iter()
        .find(|p| p.browser.identifier() == browser_id)
    else {
        return; // page closed while wl-paste ran
    };
    let Some(frame) = page
        .browser
        .focused_frame()
        .or_else(|| page.browser.main_frame())
    else {
        return;
    };
    let script = insert_text_script(&text);
    frame.execute_java_script(Some(&script.as_str().into()), Some(&"".into()), 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wl_paste_requests_text_without_newline() {
        assert_eq!(wl_paste_args(), ["--no-newline", "--type", "text"]);
    }

    #[test]
    fn insert_text_script_escapes_payload() {
        assert_eq!(
            insert_text_script("a'b\\c\nd"),
            "document.execCommand('insertText', false, 'a\\'b\\\\c\\nd');"
        );
    }

    #[test]
    fn poll_paste_waits_then_delivers_once() {
        let slot = Mutex::new(None);
        let (tx, rx) = mpsc::channel();
        *slot.lock().unwrap() = Some(PendingPaste { browser_id: 7, rx });
        assert_eq!(poll_paste(&slot), None);
        tx.send(Some("hi".to_string())).unwrap();
        assert_eq!(poll_paste(&slot), Some((7, "hi".to_string())));
        assert!(slot.lock().unwrap().is_none());
        assert_eq!(poll_paste(&slot), None);
    }

    #[test]
    fn poll_paste_clears_empty_and_dead_reads() {
        let slot = Mutex::new(None);
        let (tx, rx) = mpsc::channel();
        *slot.lock().unwrap() = Some(PendingPaste { browser_id: 1, rx });
        tx.send(None).unwrap();
        assert_eq!(poll_paste(&slot), None);
        assert!(slot.lock().unwrap().is_none());

        let (tx, rx) = mpsc::channel::<Option<String>>();
        *slot.lock().unwrap() = Some(PendingPaste { browser_id: 1, rx });
        drop(tx);
        assert_eq!(poll_paste(&slot), None);
        assert!(slot.lock().unwrap().is_none());
    }
}
