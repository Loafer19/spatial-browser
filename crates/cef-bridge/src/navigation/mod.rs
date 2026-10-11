// Custom-scheme intercepts (`bookmark://`, `omnibox://`, …): cancel in
// on_before_browse, stash in PENDING_* for the compositor (clipboard://copy
// is handled immediately — no Session needed — behind clipboard::copy_allowed).

use crate::blocklist::{OsrResourceRequestHandler, ResourceRequestHandlerBuilder};
use crate::clean_urls;
use crate::clipboard;
use cef::{self, rc::Rc, *};

mod context;
mod find;
mod pages;
mod password;
mod query;
#[cfg(test)]
mod tests;

pub use context::*;
pub use find::*;
pub use pages::*;
pub use password::*;
use query::query_param;

/// Parse `clipboard://copy?text=...` (OSR has no OS clipboard surface).
fn parse_clipboard_copy(url: &str) -> Option<String> {
    let query = url.strip_prefix("clipboard://copy?")?;
    query_param(query, "text")
}

#[derive(Clone)]
pub struct OsrRequestHandler {}

wrap_request_handler! {
    pub struct RequestHandlerBuilder {
        handler: OsrRequestHandler,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let (Some(browser), Some(request)) = (browser, request) else {
                return false as _;
            };
            let url = cef::CefString::from(&request.url()).to_string();
            let id = browser.identifier();
            if crate::spatial::is_spatial_url(&url) {
                let is_main = frame.as_ref().is_some_and(|f| f.is_main() != 0);
                let current = frame
                    .as_ref()
                    .map(|f| cef::CefString::from(&f.url()).to_string())
                    .unwrap_or_default();
                let direct = request.transition_type().get_raw()
                    & TransitionType::DIRECT_LOAD_FLAG.get_raw()
                    != 0;
                if crate::spatial::navigation_allowed(is_main, direct, &current) {
                    return false as _;
                }
                log::warn!("refused spatial:// navigation in browser {id} (main frame {is_main})");
                return true as _;
            }
            // Skip fake action schemes; they never carry tracking params.
            if url.starts_with("http://") || url.starts_with("https://") {
                if let Some(cleaned) = clean_urls::clean(&url) {
                    if let Some(frame) = frame {
                        frame.load_url(Some(&cleaned.as_str().into()));
                    }
                    return true as _; // cancel — the load_url above replaces it
                }
            }
            if let Some(action) = parse_bookmark_action(&url) {
                PENDING_BOOKMARK.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _; // cancel navigation — the compositor acts on it instead
            }
            if let Some(submit) = parse_omnibox_submit(&url) {
                PENDING_OMNIBOX.with_borrow_mut(|pending| *pending = Some((id, submit)));
                return true as _;
            }
            if let Some(query) = parse_omnibox_suggest(&url) {
                PENDING_OMNIBOX_SUGGEST.with_borrow_mut(|pending| *pending = Some((id, query)));
                return true as _;
            }
            if let Some(target_id) = parse_switch_target(&url) {
                PENDING_SWITCH.with_borrow_mut(|pending| *pending = Some((id, target_id)));
                return true as _;
            }
            if let Some(action) = parse_download_action(&url) {
                PENDING_DOWNLOAD_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_history_action(&url) {
                PENDING_HISTORY_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_workspace_action(&url) {
                PENDING_WORKSPACE_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_settings_action(&url) {
                PENDING_SETTINGS_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_userscripts_action(&url) {
                PENDING_USERSCRIPT_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_password_action(&url) {
                let frame = frame.as_deref();
                let ok = frame.is_some_and(|f| {
                    let parent_is_main = f.parent().is_some_and(|p| p.is_main() != 0);
                    let current = cef::CefString::from(&f.url()).to_string();
                    signal_frame_ok(f.is_main() != 0, parent_is_main, &current)
                });
                if ok {
                    PENDING_PASSWORD_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                } else {
                    log::warn!("dropped password:// from a subframe of browser {id}");
                }
                return true as _;
            }
            if let Some(action) = parse_context_action(&url) {
                PENDING_CONTEXT_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_find_action(&url) {
                PENDING_FIND_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(text) = parse_clipboard_copy(&url) {
                let is_main = frame.is_some_and(|f| f.is_main() != 0);
                if clipboard::copy_allowed(
                    user_gesture != 0,
                    is_main,
                    clipboard::last_copy_shortcut(),
                    std::time::Instant::now(),
                ) {
                    clipboard::copy_text(&text, false);
                } else {
                    log::warn!("dropped clipboard://copy from browser {id}: no user gesture or Ctrl+C");
                }
                return true as _; // always cancel — never a real navigation
            }
            false as _
        }

        // Blocklist applies to every request, not only top-level navigations.
        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<cef::ResourceRequestHandler> {
            Some(ResourceRequestHandlerBuilder::build(
                OsrResourceRequestHandler {},
            ))
        }
    }
}

impl RequestHandlerBuilder {
    pub fn build(handler: OsrRequestHandler) -> cef::RequestHandler {
        Self::new(handler)
    }
}
