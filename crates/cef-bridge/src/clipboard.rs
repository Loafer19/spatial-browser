// System clipboard writes (wl-copy) and the gate for page-initiated
// `clipboard://copy`. Lives in cef-bridge so navigation.rs and the
// compositor share one wl-copy path.

use std::cell::Cell;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TEXT_MIME: &str = "text/plain;charset=utf-8";

/// Args for `wl-copy`. The text never goes on argv: argv text starting with
/// `-` is parsed as flags, and argv is world-readable in /proc/*/cmdline for
/// as long as the forked wl-copy serves the selection.
fn wl_copy_args(sensitive: bool) -> Vec<&'static str> {
    // Explicit type: wl-copy otherwise sniffs content and can offer text as
    // application/* or text/uri-list.
    let mut args = vec!["--type", TEXT_MIME];
    if sensitive {
        // Clipboard-manager hint to skip history (wl-clipboard >= 2.3).
        args.push("--sensitive");
    }
    args
}

fn wl_copy_command(sensitive: bool) -> Command {
    let mut cmd = Command::new("wl-copy");
    cmd.args(wl_copy_args(sensitive))
        .stdin(Stdio::piped())
        .stdout(Stdio::null());
    cmd
}

/// Pipe `text` into one wl-copy run; `Ok(true)` = wl-copy exited 0.
fn run_wl_copy(text: &str, sensitive: bool) -> std::io::Result<bool> {
    let mut child = wl_copy_command(sensitive).spawn()?;
    let write_result = child
        .stdin
        .take()
        .map_or(Ok(()), |mut stdin| stdin.write_all(text.as_bytes()));
    // stdin dropped above → EOF; wl-copy forks a server child and the parent
    // exits, so this wait is short and reaps the parent (no zombies).
    let status = child.wait()?;
    write_result?;
    Ok(status.success())
}

/// Copy `text` to the Wayland clipboard. Returns immediately: the write and
/// wait happen on a short-lived thread so a slow compositor or a large
/// payload (pipe buffer full) never stalls the UI thread. `sensitive` adds
/// `--sensitive` and falls back to a plain copy on wl-clipboard < 2.3.
pub fn copy_text(text: &str, sensitive: bool) {
    let text = text.to_owned();
    let spawned = std::thread::Builder::new()
        .name("wl-copy".into())
        .spawn(move || {
            let mut result = run_wl_copy(&text, sensitive);
            if sensitive && !matches!(result, Ok(true)) {
                result = run_wl_copy(&text, false);
            }
            match result {
                Ok(true) => {}
                Ok(false) => log::warn!("wl-copy exited with an error; clipboard not updated"),
                Err(e) => log::warn!("wl-copy failed (system clipboard copy unavailable): {e}"),
            }
        });
    if let Err(e) = spawned {
        log::warn!("couldn't start wl-copy thread: {e}");
    }
}

// --- Gate for page-initiated clipboard://copy ---

/// How long after a real Ctrl+C / Ctrl+X a page may write the clipboard.
/// The copy event and the bridge's navigation land within milliseconds;
/// 1 s absorbs a busy renderer.
const COPY_KEY_WINDOW: Duration = Duration::from_secs(1);

thread_local! {
    // CEF UI thread == compositor main thread (same as PENDING_*).
    static LAST_COPY_KEY: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Compositor calls this when it forwards Ctrl+C / Ctrl+X to a page.
pub fn note_copy_shortcut() {
    LAST_COPY_KEY.with(|t| t.set(Some(Instant::now())));
}

pub(crate) fn last_copy_shortcut() -> Option<Instant> {
    LAST_COPY_KEY.with(Cell::get)
}

/// May a `clipboard://copy` navigation write the system clipboard?
///
/// Without a gate any page can `location='clipboard://copy?text=…'` at any
/// time (pastejacking). Two signals count as user intent; either suffices:
///
/// - `user_gesture`: CEF's OnBeforeBrowse flag is Chromium's transient user
///   activation of the initiating frame. Clicks always set it (the passwords
///   page Copy button, page "copy" buttons that run execCommand('copy')).
///   This matches Chrome's own rule for clipboard writes.
/// - A real Ctrl+C / Ctrl+X seen by the compositor within COPY_KEY_WINDOW.
///   Keydown should also grant activation, but whether an OSR-injected
///   keydown with Ctrl held reaches Blink as activation-triggering depends on
///   how the key event is synthesized (dom_key, modifiers), and a false
///   negative would silently break Ctrl+C everywhere. The compositor sees the
///   physical key before CEF, so this signal cannot be forged by the page.
///
/// Main frame only: the copy bridge is injected into the main frame, so a
/// subframe sending clipboard://copy is an iframe (e.g. an ad) acting alone.
pub(crate) fn copy_allowed(
    user_gesture: bool,
    is_main_frame: bool,
    last_copy_key: Option<Instant>,
    now: Instant,
) -> bool {
    if !is_main_frame {
        return false;
    }
    let recent_copy_key = last_copy_key.is_some_and(|t| {
        now.checked_duration_since(t)
            .is_some_and(|d| d <= COPY_KEY_WINDOW)
    });
    user_gesture || recent_copy_key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wl_copy_text_goes_to_stdin_not_argv() {
        let cmd = wl_copy_command(false);
        assert_eq!(cmd.get_program(), "wl-copy");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, ["--type", TEXT_MIME]);
    }

    #[test]
    fn wl_copy_sensitive_adds_hint() {
        assert_eq!(wl_copy_args(true), ["--type", TEXT_MIME, "--sensitive"]);
    }

    #[test]
    fn copy_needs_gesture_or_recent_copy_key() {
        let now = Instant::now();
        let just_now = now.checked_sub(Duration::from_millis(50));
        let stale = now.checked_sub(Duration::from_secs(5));

        // Drive-by navigation: no gesture, no key.
        assert!(!copy_allowed(false, true, None, now));
        assert!(!copy_allowed(false, true, stale, now));
        // Click (gesture) or recent Ctrl+C.
        assert!(copy_allowed(true, true, None, now));
        assert!(copy_allowed(false, true, just_now, now));
        // Window edge is inclusive; key "in the future" (clock skew) is not.
        assert!(copy_allowed(
            false,
            true,
            now.checked_sub(COPY_KEY_WINDOW),
            now
        ));
        assert!(!copy_allowed(
            false,
            true,
            Some(now + Duration::from_secs(1)),
            now
        ));
    }

    #[test]
    fn copy_from_subframe_is_rejected() {
        let now = Instant::now();
        assert!(!copy_allowed(true, false, Some(now), now));
    }

    #[test]
    fn note_copy_shortcut_records_time() {
        let before = Instant::now();
        note_copy_shortcut();
        assert!(last_copy_shortcut().is_some_and(|t| t >= before));
    }
}
