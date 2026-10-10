// Cancel popups so CEF does not open native GTK/XWayland windows; compositor
// spawns canvas Pages from PENDING_POPUPS instead.
//
// The compositor respawns each popup as a fresh, browser-initiated page, which
// skips Chromium's own guards on renderer-initiated top-level navigations
// (data:/file:/javascript:) and its popup blocker. So the filter lives here:
// user gesture required, http(s) only, and at most one popup per opener per
// POPUP_MIN_INTERVAL. Everything else is cancelled and logged.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// One popup per opener page per this interval. A `target=_blank` click or an
/// OAuth `window.open` is one popup per gesture; a loop of `window.open` calls
/// inside a single click handler gets the first one only.
const POPUP_MIN_INTERVAL: Duration = Duration::from_secs(1);

thread_local! {
    // Queue: multiple window.open() calls can land in one frame.
    pub static PENDING_POPUPS: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
    // Opener browser id → when its last popup was accepted.
    static LAST_POPUP: RefCell<HashMap<i32, Instant>> = RefCell::new(HashMap::new());
}

#[derive(Debug, PartialEq, Eq)]
enum PopupVerdict {
    Allow,
    NoGesture,
    BadScheme,
    RateLimited,
}

/// Only `http://` and `https://` targets become canvas pages. Chromium hands
/// us a canonicalized URL, so the scheme is already lowercase, but compare
/// case-insensitively anyway.
fn popup_url_allowed(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once(':') else {
        return false;
    };
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && rest.starts_with("//")
        && rest.len() > 2
}

fn popup_rate_ok(last: Option<Instant>, now: Instant) -> bool {
    match last {
        Some(last) => now.saturating_duration_since(last) >= POPUP_MIN_INTERVAL,
        None => true,
    }
}

fn popup_verdict(user_gesture: bool, url: &str, last: Option<Instant>, now: Instant) -> PopupVerdict {
    if !user_gesture {
        PopupVerdict::NoGesture
    } else if !popup_url_allowed(url) {
        PopupVerdict::BadScheme
    } else if !popup_rate_ok(last, now) {
        PopupVerdict::RateLimited
    } else {
        PopupVerdict::Allow
    }
}

/// Decide and, if allowed, record the popup time for `opener_id`.
fn admit_popup(opener_id: i32, user_gesture: bool, url: &str) -> PopupVerdict {
    let now = Instant::now();
    LAST_POPUP.with_borrow_mut(|last_popup| {
        let verdict = popup_verdict(user_gesture, url, last_popup.get(&opener_id).copied(), now);
        if verdict == PopupVerdict::Allow {
            // Drop stale entries so closed openers don't accumulate.
            last_popup.retain(|_, t| !popup_rate_ok(Some(*t), now));
            last_popup.insert(opener_id, now);
        }
        verdict
    })
}

#[derive(Clone)]
pub struct OsrLifeSpanHandler {}

wrap_life_span_handler! {
    pub struct LifeSpanHandlerBuilder {
        handler: OsrLifeSpanHandler,
    }

    impl LifeSpanHandler {
        // Always returns true: cancel CEF's default native popup window, then
        // queue the accepted ones for the compositor.
        fn on_before_popup(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let Some(browser) = browser else {
                log::warn!("popup blocked: no opener browser");
                return true as _;
            };
            let url = target_url.map(|u| u.to_string()).unwrap_or_default();
            let opener_id = browser.identifier();
            match admit_popup(opener_id, user_gesture != 0, &url) {
                PopupVerdict::Allow => {
                    PENDING_POPUPS.with_borrow_mut(|pending| pending.push((opener_id, url)));
                }
                verdict => {
                    log::info!("popup blocked ({verdict:?}) from browser {opener_id}: {url:?}");
                }
            }
            true as _
        }

        // Every close path ends here: drop the tile's built-in page HTML.
        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                crate::spatial::forget_browser(browser.identifier());
            }
        }
    }
}

impl LifeSpanHandlerBuilder {
    pub fn build(handler: OsrLifeSpanHandler) -> cef::LifeSpanHandler {
        Self::new(handler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_url_scheme_filter() {
        assert!(popup_url_allowed("https://accounts.example.test/o/oauth2/auth?x=1"));
        assert!(popup_url_allowed("http://example.test/"));
        assert!(popup_url_allowed("HTTPS://example.test/"));
        assert!(!popup_url_allowed(""));
        assert!(!popup_url_allowed("about:blank"));
        assert!(!popup_url_allowed("data:text/html,<script>alert(1)</script>"));
        assert!(!popup_url_allowed("javascript:alert(1)"));
        assert!(!popup_url_allowed("file:///etc/passwd"));
        assert!(!popup_url_allowed("blob:https://example.test/uuid"));
        assert!(!popup_url_allowed("chrome://settings"));
        assert!(!popup_url_allowed("bookmark://open/1"));
        assert!(!popup_url_allowed("https:"));
        assert!(!popup_url_allowed("https://"));
        assert!(!popup_url_allowed("httpx://example.test/"));
    }

    #[test]
    fn popup_rate_limit() {
        let t0 = Instant::now();
        assert!(popup_rate_ok(None, t0));
        assert!(!popup_rate_ok(Some(t0), t0));
        assert!(!popup_rate_ok(Some(t0), t0 + Duration::from_millis(999)));
        assert!(popup_rate_ok(Some(t0), t0 + POPUP_MIN_INTERVAL));
        // Clock oddities (last in the future) must not panic and stay blocked.
        assert!(!popup_rate_ok(Some(t0 + Duration::from_secs(5)), t0));
    }

    #[test]
    fn popup_verdict_order() {
        let t0 = Instant::now();
        let url = "https://example.test/";
        assert_eq!(popup_verdict(true, url, None, t0), PopupVerdict::Allow);
        assert_eq!(popup_verdict(false, url, None, t0), PopupVerdict::NoGesture);
        assert_eq!(popup_verdict(true, "data:,x", None, t0), PopupVerdict::BadScheme);
        assert_eq!(popup_verdict(true, url, Some(t0), t0), PopupVerdict::RateLimited);
    }

    #[test]
    fn admit_popup_limits_per_opener() {
        let url = "https://example.test/";
        assert_eq!(admit_popup(9001, true, url), PopupVerdict::Allow);
        assert_eq!(admit_popup(9001, true, url), PopupVerdict::RateLimited);
        // Another opener has its own budget.
        assert_eq!(admit_popup(9002, true, url), PopupVerdict::Allow);
        // Rejected popups don't consume the budget.
        assert_eq!(admit_popup(9003, false, url), PopupVerdict::NoGesture);
        assert_eq!(admit_popup(9003, true, url), PopupVerdict::Allow);
    }
}
