// Per-browser paint counter for the compositor's frame pacer. Paint
// callbacks run on the CEF UI thread, which is the compositor's main thread
// under external_message_pump, so a thread_local is enough.
//
// The compositor also notices paints by the page's bind group changing; a
// paint path that reuses its bind group must call `note_paint` or the pacer
// will treat the page as idle (see compositor frame_pacer.rs).

use std::cell::RefCell;
use std::collections::HashMap;

// One u64 per browser ever created; small enough not to prune.
thread_local! {
    static PAINTS: RefCell<HashMap<i32, u64>> = RefCell::new(HashMap::new());
}

/// Record one paint for `browser_id` (call from on_paint / on_accelerated_paint).
pub fn note_paint(browser_id: i32) {
    PAINTS.with_borrow_mut(|paints| {
        let count = paints.entry(browser_id).or_insert(0);
        *count = count.wrapping_add(1);
    });
}

/// Paints recorded for `browser_id` so far (0 if none).
pub fn paint_count(browser_id: i32) -> u64 {
    PAINTS.with_borrow(|paints| paints.get(&browser_id).copied().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_per_browser() {
        assert_eq!(paint_count(7), 0);
        note_paint(7);
        note_paint(7);
        note_paint(8);
        assert_eq!(paint_count(7), 2);
        assert_eq!(paint_count(8), 1);
    }
}
