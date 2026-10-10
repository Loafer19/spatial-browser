// External message pump schedule. CEF calls on_schedule_message_pump_work
// (any thread) to say when do_message_loop_work is next due; the embedder
// loop sleeps until then and a registered waker interrupts that sleep when
// the deadline moves earlier. See main.rs for the loop that consumes this.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Never trust CEF to ask for more than this (cefclient's kMaxTimerDelay).
pub const MAX_PUMP_DELAY: Duration = Duration::from_millis(1000 / 30);

type Waker = Box<dyn Fn() + Send + Sync>;

static NEXT_WORK: Mutex<Option<Instant>> = Mutex::new(None);
static WAKER: OnceLock<Waker> = OnceLock::new();

/// Install the event-loop wake callback (once; later calls are ignored).
pub fn set_message_pump_waker(waker: impl Fn() + Send + Sync + 'static) {
    let _ = WAKER.set(Box::new(waker));
}

/// Deadline CEF last asked for, if any work is pending.
pub fn next_message_pump_work() -> Option<Instant> {
    NEXT_WORK.lock().ok().and_then(|slot| *slot)
}

/// True (and clears the request) when scheduled work is due at `now`.
pub fn take_message_pump_work(now: Instant) -> bool {
    let Ok(mut slot) = NEXT_WORK.lock() else {
        return true;
    };
    match *slot {
        Some(deadline) if deadline <= now => {
            *slot = None;
            true
        }
        _ => false,
    }
}

/// CEF semantics: a new request replaces any pending one.
pub(crate) fn schedule(delay_ms: i64) {
    let now = Instant::now();
    let deadline = now + delay_from_ms(delay_ms);
    let earlier = match NEXT_WORK.lock() {
        Ok(mut slot) => {
            let earlier = slot.is_none_or(|current| deadline < current);
            *slot = Some(deadline);
            earlier
        }
        Err(_) => true,
    };
    // A later deadline needs no wake: the loop already sleeps less than that.
    if earlier {
        if let Some(wake) = WAKER.get() {
            wake();
        }
    }
}

fn delay_from_ms(delay_ms: i64) -> Duration {
    if delay_ms <= 0 {
        Duration::ZERO
    } else {
        Duration::from_millis(delay_ms as u64).min(MAX_PUMP_DELAY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_is_clamped() {
        assert_eq!(delay_from_ms(-5), Duration::ZERO);
        assert_eq!(delay_from_ms(0), Duration::ZERO);
        assert_eq!(delay_from_ms(5), Duration::from_millis(5));
        assert_eq!(delay_from_ms(60_000), MAX_PUMP_DELAY);
    }

    #[test]
    fn schedule_then_take() {
        schedule(0);
        assert!(next_message_pump_work().is_some());
        assert!(take_message_pump_work(Instant::now()));
        assert!(!take_message_pump_work(Instant::now()));
        schedule(20);
        assert!(!take_message_pump_work(Instant::now()));
        assert!(take_message_pump_work(
            Instant::now() + Duration::from_millis(40)
        ));
    }
}
