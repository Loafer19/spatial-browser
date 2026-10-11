// Downloads: finished ones (record + notification), live progress pushed to
// open downloads tiles, and `download://` actions from the downloads page.

use super::*;
use cef_bridge::{DownloadChanges, DownloadControl, PENDING_DOWNLOAD_CHANGES};
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// Byte/speed pushes go out at most this often (state changes at once).
const PROGRESS_PUSH_INTERVAL: Duration = Duration::from_millis(250);

thread_local! {
    static LIVE_PUSH: RefCell<PushThrottle> = RefCell::new(PushThrottle::default());
}

/// Rate limit for live pushes. Every downloads tile is pushed at the same
/// moments, so this bounds each tile to ~4 progress pushes a second.
#[derive(Default)]
struct PushThrottle {
    /// Progress arrived that no push has carried yet.
    dirty: bool,
    last: Option<Instant>,
}

impl PushThrottle {
    /// Whether to push now. `state` (start, pause, finish…) always goes out.
    fn due(&mut self, now: Instant, changes: DownloadChanges) -> bool {
        self.dirty |= changes.progress;
        let due = changes.state
            || (self.dirty
                && self
                    .last
                    .is_none_or(|last| now.duration_since(last) >= PROGRESS_PUSH_INTERVAL));
        if due {
            self.dirty = false;
            self.last = Some(now);
        }
        due
    }

    /// No tile to push to: forget pending progress (a tile opened later
    /// renders the current state itself).
    fn idle(&mut self) {
        self.dirty = false;
    }
}

/// A built-in downloads tile: right kind, token issued to this very browser.
fn is_downloads_tile(ephemeral: bool, url: &str, browser_id: i32) -> bool {
    pages::kind_of(url) == Some(pages::kind::DOWNLOADS)
        && is_internal_page(ephemeral, url, browser_id)
}

fn downloads_tiles(session: &Session) -> Vec<&cef::Browser> {
    session
        .pages()
        .iter()
        .filter(|p| is_downloads_tile(p.ephemeral, &p.url(), p.browser.identifier()))
        .map(|p| &p.browser)
        .collect()
}

/// Push the live list (and the completed list with `records`) to every open
/// downloads tile, throttled. Nothing happens without an open tile.
fn push_live(session: &Session, changes: DownloadChanges, records: Option<&[DownloadRecord]>) {
    let tiles = downloads_tiles(session);
    let due = LIVE_PUSH.with_borrow_mut(|throttle| {
        if tiles.is_empty() {
            throttle.idle();
            false
        } else {
            throttle.due(Instant::now(), changes)
        }
    });
    if !due {
        return;
    }
    let json = pages::downloads_list::live_json(&cef_bridge::active_downloads(), records);
    let js = pages::ui::push_js("downloads", &json);
    for browser in tiles {
        pages::ui::run_js(browser, &js);
    }
}

/// Small feedback line in the downloads tile `browser_id`.
fn toast(session: &Session, browser_id: i32, text: &str) {
    if let Some(page) = session
        .pages()
        .iter()
        .find(|p| p.browser.identifier() == browser_id)
    {
        let js = format!("window.ui&&ui.toast({});", pages::js_string_literal(text));
        pages::ui::run_js(&page.browser, &js);
    }
}

fn xdg_open(target: &std::path::Path) {
    if let Err(e) = std::process::Command::new("xdg-open").arg(target).spawn() {
        log::warn!("xdg-open failed for {target:?}: {e}");
    }
}

/// Finished downloads (record + notification), live pushes, and
/// `download://` list actions.
pub(super) fn drain(session: &Session, downloads: &mut Vec<DownloadRecord>) {
    // Completed downloads → downloads.json + desktop notification.
    let completed = PENDING_DOWNLOADS.with_borrow_mut(std::mem::take);
    let recorded = !completed.is_empty();
    for download in completed {
        let filename = std::path::Path::new(&download.path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&download.path)
            .to_string();
        downloads::record(
            downloads,
            DownloadRecord {
                url: download.url,
                path: download.path,
                mime: Some(download.mime).filter(|m| !m.is_empty()),
            },
        );
        if let Err(e) = std::process::Command::new("notify-send")
            .arg("Download complete")
            .arg(&filename)
            .spawn()
        {
            log::warn!("notify-send failed (download {filename:?} still saved): {e}");
        }
    }

    // Progress / state → open downloads tiles. A finished download leaves
    // the active list and joins the completed one in the same push.
    // Runs every tick, also with no news: it flushes throttled progress.
    let mut changes = PENDING_DOWNLOAD_CHANGES.with_borrow_mut(std::mem::take);
    changes.state |= recorded;
    push_live(session, changes, recorded.then_some(downloads.as_slice()));

    // download://…
    if let Some((browser_id, action)) = PENDING_DOWNLOAD_ACTION
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "download://"))
    {
        match action {
            DownloadPageAction::Open(index) => {
                if let Some(download) = downloads.get(index) {
                    // File for safe types, its folder for executables/scripts/
                    // unknown; None if the record points outside ~/Downloads.
                    match cef_bridge::download_open_target(&download.path) {
                        Some(target) => xdg_open(&target),
                        None => {
                            log::warn!(
                                "refused to open download {:?}: missing or outside downloads dir",
                                download.path
                            );
                            toast(session, browser_id, "File is missing or was moved");
                        }
                    }
                }
            }
            DownloadPageAction::Reveal(index) => {
                if let Some(download) = downloads.get(index) {
                    xdg_open(&cef_bridge::download_folder_target(&download.path));
                }
            }
            DownloadPageAction::Remove(index) => {
                if index < downloads.len() {
                    downloads.remove(index);
                    downloads::save(downloads);
                }
                // Indices shifted: every tile gets the whole list again.
                let changes = DownloadChanges {
                    progress: false,
                    state: true,
                };
                push_live(session, changes, Some(downloads));
            }
            DownloadPageAction::Cancel(id) => control(id, DownloadControl::Cancel),
            DownloadPageAction::Pause(id) => control(id, DownloadControl::Pause),
            DownloadPageAction::Resume(id) => control(id, DownloadControl::Resume),
        }
    }
}

/// The resulting state change reaches the page through the next push.
fn control(id: u32, control: DownloadControl) {
    if !cef_bridge::control_download(id, control) {
        log::debug!("download {id} is no longer active; ignored {control:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRESS: DownloadChanges = DownloadChanges {
        progress: true,
        state: false,
    };
    const STATE: DownloadChanges = DownloadChanges {
        progress: false,
        state: true,
    };

    #[test]
    fn throttle_caps_progress_and_never_delays_state() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut throttle = PushThrottle::default();
        assert!(throttle.due(ms(0), PROGRESS));
        // Within the interval: held back, but remembered.
        assert!(!throttle.due(ms(100), PROGRESS));
        assert!(!throttle.due(ms(200), DownloadChanges::default()));
        // Flushed once the interval has passed, even with nothing new.
        assert!(throttle.due(ms(250), DownloadChanges::default()));
        assert!(!throttle.due(ms(600), DownloadChanges::default()));
        // State changes go out immediately, and reset the clock.
        assert!(throttle.due(ms(610), STATE));
        assert!(!throttle.due(ms(700), PROGRESS));
        assert!(throttle.due(ms(700), STATE));
        // That push carried the pending progress too.
        assert!(!throttle.due(ms(1_000), DownloadChanges::default()));

        // At most ~4 progress pushes a second under a constant stream.
        let mut throttle = PushThrottle::default();
        let pushes = (0..1_000)
            .filter(|n| throttle.due(ms(10_000 + n), PROGRESS))
            .count();
        assert_eq!(pushes, 4);
    }

    #[test]
    fn idle_drops_pending_progress() {
        let t0 = Instant::now();
        let mut throttle = PushThrottle::default();
        assert!(throttle.due(t0, PROGRESS));
        assert!(!throttle.due(t0, PROGRESS));
        throttle.idle();
        assert!(!throttle.due(t0 + Duration::from_secs(1), DownloadChanges::default()));
    }

    #[test]
    fn only_issued_downloads_tiles_get_pushes() {
        let url = pages::serve(pages::kind::DOWNLOADS, String::new());
        // Not yet bound to any browser.
        assert!(!is_downloads_tile(true, &url, 41_001));
        assert!(cef_bridge::spatial::bind(&url, 41_001));
        assert!(is_downloads_tile(true, &url, 41_001));
        // A web tile showing it, or another browser.
        assert!(!is_downloads_tile(false, &url, 41_001));
        assert!(!is_downloads_tile(true, &url, 41_002));
        // Another built-in kind, bound and internal, is not a downloads tile.
        let help = pages::serve(pages::kind::HELP, String::new());
        assert!(cef_bridge::spatial::bind(&help, 41_003));
        assert!(!is_downloads_tile(true, &help, 41_003));
        // Web pages.
        assert!(!is_downloads_tile(true, "https://downloads.test/", 41_001));
        cef_bridge::spatial::forget_browser(41_001);
        cef_bridge::spatial::forget_browser(41_003);
        assert!(!is_downloads_tile(true, &url, 41_001));
    }
}
