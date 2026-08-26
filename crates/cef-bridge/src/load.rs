// Visit tracking + userscript inject hooks: every top-level load start
// and load end gets queued for the compositor (document-start /
// document-end userscripts, history recording).

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

thread_local! {
    // Appended by OsrLoadHandler::on_load_end for every completed
    // main-frame navigation — drained once per frame by the compositor
    // to record real browsing history (persistence::history, distinct
    // from typed_history.rs — what was actually typed into the
    // omnibox) and to inject document-end / document-idle userscripts.
    // A queue, not a single slot: more than one page can finish loading
    // within the same frame.
    pub static PENDING_VISITS: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };

    // Same shape for on_load_start — document-start userscripts need to
    // run before the page's own scripts; on_load_end is too late for that.
    pub static PENDING_LOAD_START: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone)]
pub struct OsrLoadHandler {}

wrap_load_handler! {
    pub struct LoadHandlerBuilder {
        handler: OsrLoadHandler,
    }

    impl LoadHandler {
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
