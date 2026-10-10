// Find-in-page: CEF's native BrowserHost::find reports matches here; the
// compositor drains PENDING_FIND_RESULTS into the find bar. Bar actions
// (`find://go/...`) are parsed in navigation.rs.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindResult {
    pub count: i32,
    /// 1-based index of the highlighted match; 0 when none.
    pub active: i32,
    pub is_final: bool,
}

thread_local! {
    // (browser_id of the searched page, result)
    pub static PENDING_FIND_RESULTS: RefCell<Vec<(i32, FindResult)>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone)]
pub struct OsrFindHandler {}

wrap_find_handler! {
    pub struct FindHandlerBuilder {
        handler: OsrFindHandler,
    }

    impl FindHandler {
        fn on_find_result(
            &self,
            browser: Option<&mut Browser>,
            _identifier: ::std::os::raw::c_int,
            count: ::std::os::raw::c_int,
            _selection_rect: Option<&Rect>,
            active_match_ordinal: ::std::os::raw::c_int,
            final_update: ::std::os::raw::c_int,
        ) {
            let Some(browser) = browser else { return };
            let result = FindResult {
                count,
                active: active_match_ordinal,
                is_final: final_update != 0,
            };
            PENDING_FIND_RESULTS.with_borrow_mut(|q| q.push((browser.identifier(), result)));
        }
    }
}

impl FindHandlerBuilder {
    pub fn build(handler: OsrFindHandler) -> FindHandler {
        Self::new(handler)
    }
}
