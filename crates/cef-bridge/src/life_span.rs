// Popup interception: a page trying to open a link in a new tab/window
// (target="_blank", window.open(), middle-click) gets canceled here so
// CEF never falls back to its own default popup handling — in this
// windowless/OSR embedding, with no LifeSpanHandler override at all,
// that default is a real native (GTK/XWayland) top-level window,
// positioned and managed by the window manager rather than the canvas,
// with no Session/hotkeys awareness of it at all.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

thread_local! {
    // Appended by OsrLifeSpanHandler::on_before_popup when a page tries
    // to open a link in a new tab/window (target="_blank", window.open,
    // middle-click on a link) — canceled there so CEF doesn't spawn its
    // own native popup window outside the canvas entirely untracked by
    // Session; the compositor spawns a regular Page instead. A queue,
    // not a single slot: a page can fire more than one popup request in
    // the same frame (e.g. several window.open() calls in a row).
    pub static PENDING_POPUPS: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone)]
pub struct OsrLifeSpanHandler {}

wrap_life_span_handler! {
    pub struct LifeSpanHandlerBuilder {
        handler: OsrLifeSpanHandler,
    }

    impl LifeSpanHandler {
        // Canceling (returning true) here is what stops CEF from falling
        // back to its default popup handling — in a windowless/OSR
        // embedding with no LifeSpanHandler override at all, that default
        // is a real native (GTK/XWayland) top-level window, positioned
        // and managed by the window manager rather than our canvas.
        fn on_before_popup(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let (Some(browser), Some(target_url)) = (browser, target_url) else {
                return false as _;
            };
            let url = target_url.to_string();
            if url.is_empty() {
                return false as _;
            }
            PENDING_POPUPS.with_borrow_mut(|pending| pending.push((browser.identifier(), url)));
            true as _
        }
    }
}

impl LifeSpanHandlerBuilder {
    pub fn build(handler: OsrLifeSpanHandler) -> cef::LifeSpanHandler {
        Self::new(handler)
    }
}
