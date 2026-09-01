// Cancel popups so CEF does not open native GTK/XWayland windows; compositor
// spawns canvas Pages from PENDING_POPUPS instead.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

thread_local! {
    // Queue: multiple window.open() calls can land in one frame.
    pub static PENDING_POPUPS: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone)]
pub struct OsrLifeSpanHandler {}

wrap_life_span_handler! {
    pub struct LifeSpanHandlerBuilder {
        handler: OsrLifeSpanHandler,
    }

    impl LifeSpanHandler {
        // true = cancel CEF's default native popup window.
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
