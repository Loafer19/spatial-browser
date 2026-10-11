// Deferred tile loads: restored tiles load when they come on screen, and a
// tile whose renderer crashed reloads after a backoff. Both are staggered
// (one load start per `STAGGER`) so a restore or a crash loop can't start
// a dozen heavy pages in the same frame — the "reload storm" that used to
// bring the whole app down again right after a relaunch.

use crate::output::Rect;
use crate::session::Session;
use cef::ImplBrowser;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Minimum gap between two deferred load starts.
pub const STAGGER: Duration = Duration::from_millis(150);
/// Crashes of one tile within this window count towards its backoff.
pub const CRASH_WINDOW: Duration = Duration::from_secs(10 * 60);
/// More crashes than this within `CRASH_WINDOW`: stop reloading by itself.
pub const MAX_AUTO_RELOADS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLoad {
    pub url: String,
    pub not_before: Instant,
}

impl PendingLoad {
    pub fn now(url: &str) -> Self {
        Self {
            url: url.to_string(),
            not_before: Instant::now(),
        }
    }
}

/// Delay before the automatic reload after the `n`th recent crash
/// (1-based); `None` = give up, the user reloads by hand (Ctrl+R).
pub fn crash_backoff(n: usize) -> Option<Duration> {
    match n {
        n if n > MAX_AUTO_RELOADS => None,
        0 | 1 => Some(Duration::from_secs(1)),
        2 => Some(Duration::from_secs(5)),
        _ => Some(Duration::from_secs(30)),
    }
}

/// Screen rect `r` overlaps a `w`×`h` window.
pub fn on_screen(r: Rect, (w, h): (f32, f32)) -> bool {
    r.x < w && r.y < h && r.x + r.w > 0.0 && r.y + r.h > 0.0
}

thread_local! {
    static CRASHES: RefCell<HashMap<i32, Vec<Instant>>> = RefCell::new(HashMap::new());
    static LAST_START: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

/// Record a crash of `browser_id` at `now`; returns how many fell inside
/// `CRASH_WINDOW` (this one included).
fn record_crash(browser_id: i32, now: Instant) -> usize {
    CRASHES.with_borrow_mut(|all| {
        let times = all.entry(browser_id).or_default();
        times.retain(|t| now.duration_since(*t) < CRASH_WINDOW);
        times.push(now);
        times.len()
    })
}

/// Renderer crashes reported by cef-bridge → schedule that tile's reload.
/// Only the crashed tile is touched; built-in tiles are just closed-and-
/// forgotten by the user (their tokens are single-use), so they're skipped.
pub fn note_crashes(session: &Session) {
    let crashes = cef_bridge::PENDING_RENDER_CRASHES.with_borrow_mut(std::mem::take);
    let now = Instant::now();
    for crash in crashes {
        let Some(page) = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == crash.browser_id)
        else {
            continue;
        };
        if page.ephemeral {
            continue;
        }
        let n = record_crash(crash.browser_id, now);
        page.load_active.set(false);
        match crash_backoff(n) {
            Some(delay) => {
                let url = page.url();
                log::info!(
                    "tile {} crashed ({}), reload #{n} in {delay:?}: {url}",
                    crash.browser_id,
                    crash.status
                );
                *page.pending.borrow_mut() = Some(PendingLoad {
                    url,
                    not_before: now + delay,
                });
            }
            None => log::warn!(
                "tile {} crashed {n} times in {CRASH_WINDOW:?}; not reloading it automatically (Ctrl+R)",
                crash.browser_id
            ),
        }
    }
}

/// Start at most one due, on-screen deferred load (staggered).
pub fn drain(session: &Session, screen: (f32, f32)) {
    let now = Instant::now();
    if LAST_START.with_borrow(|t| t.is_some_and(|t| now.duration_since(t) < STAGGER)) {
        return;
    }
    let viewport = session.viewport();
    // Topmost first: what the user looks at loads first.
    let Some(page) = session.pages().iter().rev().find(|p| {
        p.pending
            .borrow()
            .as_ref()
            .is_some_and(|l| l.not_before <= now)
            && on_screen(viewport.rect_to_screen(p.rect), screen)
    }) else {
        return;
    };
    let Some(load) = page.pending.borrow_mut().take() else {
        return;
    };
    LAST_START.with_borrow_mut(|t| *t = Some(now));
    if let Some(frame) = page.browser.main_frame() {
        page.load_active.set(true);
        cef::ImplFrame::load_url(&frame, Some(&load.url.as_str().into()));
    }
}

/// When the loop must wake to start the next due on-screen load (`None` =
/// nothing waiting on screen; moving the canvas wakes the loop anyway).
pub fn next_due(session: &Session, screen: (f32, f32)) -> Option<Instant> {
    let viewport = session.viewport();
    let due = session
        .pages()
        .iter()
        .filter(|p| on_screen(viewport.rect_to_screen(p.rect), screen))
        .filter_map(|p| p.pending.borrow().as_ref().map(|l| l.not_before))
        .min()?;
    let gate = LAST_START.with_borrow(|t| t.map(|t| t + STAGGER));
    Some(gate.map_or(due, |g| due.max(g)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_gives_up() {
        assert_eq!(crash_backoff(1), Some(Duration::from_secs(1)));
        assert!(crash_backoff(2) > crash_backoff(1));
        assert!(crash_backoff(3) > crash_backoff(2));
        assert_eq!(crash_backoff(MAX_AUTO_RELOADS + 1), None);
    }

    #[test]
    fn crashes_outside_the_window_are_forgotten() {
        let t0 = Instant::now();
        assert_eq!(record_crash(7, t0), 1);
        assert_eq!(record_crash(7, t0 + Duration::from_secs(1)), 2);
        assert_eq!(record_crash(8, t0), 1, "per tile");
        assert_eq!(
            record_crash(7, t0 + CRASH_WINDOW + Duration::from_secs(2)),
            1
        );
    }

    #[test]
    fn visibility_is_any_overlap() {
        let r = |x, y| Rect {
            x,
            y,
            w: 100.0,
            h: 100.0,
        };
        let screen = (800.0, 600.0);
        assert!(on_screen(r(0.0, 0.0), screen));
        assert!(on_screen(r(-50.0, 550.0), screen));
        assert!(!on_screen(r(-100.0, 0.0), screen));
        assert!(!on_screen(r(800.0, 0.0), screen));
        assert!(!on_screen(r(0.0, 600.0), screen));
    }
}
