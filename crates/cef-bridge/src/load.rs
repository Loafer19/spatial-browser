// Queue top-level load start/end for history + userscript inject timing.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

thread_local! {
    pub static PENDING_VISITS: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
    // document-start must run before page scripts; on_load_end is too late.
    pub static PENDING_LOAD_START: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
    // (browser_id, is_loading) — fallback when progress events are sparse.
    pub static PENDING_LOAD_STATE: RefCell<Vec<(i32, bool)>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone)]
pub struct OsrLoadHandler {}

wrap_load_handler! {
    pub struct LoadHandlerBuilder {
        handler: OsrLoadHandler,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            browser: Option<&mut Browser>,
            is_loading: ::std::os::raw::c_int,
            _can_go_back: ::std::os::raw::c_int,
            _can_go_forward: ::std::os::raw::c_int,
        ) {
            let Some(browser) = browser else {
                return;
            };
            PENDING_LOAD_STATE
                .with_borrow_mut(|q| q.push((browser.identifier(), is_loading != 0)));
        }

        fn on_load_start(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            let (Some(browser), Some(frame)) = (browser, frame) else {
                return;
            };
            if frame.is_main() == 0 {
                return;
            }
            let url = CefString::from(&frame.url()).to_string();
            if url.is_empty() {
                return;
            }
            PENDING_LOAD_START
                .with_borrow_mut(|pending| pending.push((browser.identifier(), url)));
        }

        fn on_load_end(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _http_status_code: ::std::os::raw::c_int,
        ) {
            let (Some(browser), Some(frame)) = (browser, frame) else {
                return;
            };
            // Only the top-level navigation counts as "a visit" — every
            // iframe/subresource on the page would otherwise also fire
            // on_load_end, flooding history with things the user never
            // navigated to themselves.
            if frame.is_main() == 0 {
                return;
            }
            let url = CefString::from(&frame.url()).to_string();
            if url.is_empty() {
                return;
            }
            PENDING_VISITS.with_borrow_mut(|pending| pending.push((browser.identifier(), url)));
        }
    }
}

impl LoadHandlerBuilder {
    pub fn build(handler: OsrLoadHandler) -> cef::LoadHandler {
        Self::new(handler)
    }
}
