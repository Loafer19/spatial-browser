// Popups. Allowed ones are created by CEF itself as windowless browsers from
// the compositor's factory (so they stay children of their opener and
// `window.opener` works) and handed over via PENDING_POPUP_BROWSERS; without a
// factory they fall back to cancel + respawn from PENDING_POPUPS. Cancelling
// is never left to CEF's default, which would open a native GTK window.
//
// The compositor respawns each popup as a fresh, browser-initiated page, which
// skips Chromium's own guards on renderer-initiated top-level navigations
// (data:/file:/javascript:) and its popup blocker. So the filter lives here:
// user gesture required, http(s) only, and at most one popup per opener per
// POPUP_MIN_INTERVAL. Everything else is cancelled and logged.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::render::OsrRenderHandles;

/// What CEF needs to create a popup as one of our windowless pages.
pub struct PopupSetup {
    pub window_info: WindowInfo,
    pub client: Client,
    pub settings: BrowserSettings,
    pub handles: OsrRenderHandles,
}

/// A popup CEF created from a `PopupSetup`, waiting to become a tile.
pub struct CreatedPopup {
    pub opener_id: i32,
    pub browser: Browser,
    pub handles: OsrRenderHandles,
}

type PopupFactory = Box<dyn Fn() -> PopupSetup>;

/// A setup whose browser hasn't shown up in on_after_created after this
/// long belongs to a popup CEF dropped; it must not be matched to a later one.
const POPUP_CREATE_TIMEOUT: Duration = Duration::from_secs(5);

/// One popup per opener page per this interval. A `target=_blank` click or an
/// OAuth `window.open` is one popup per gesture; a loop of `window.open` calls
/// inside a single click handler gets the first one only.
const POPUP_MIN_INTERVAL: Duration = Duration::from_secs(1);

thread_local! {
    // Queue: multiple window.open() calls can land in one frame.
    pub static PENDING_POPUPS: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
    // Opener browser id → when its last popup was accepted.
    static LAST_POPUP: RefCell<HashMap<i32, Instant>> = RefCell::new(HashMap::new());
    static POPUP_FACTORY: RefCell<Option<PopupFactory>> = const { RefCell::new(None) };
    // Setups handed to CEF, in order, until their browser is created.
    static AWAITING_POPUPS: RefCell<VecDeque<(Instant, i32, OsrRenderHandles)>> = const { RefCell::new(VecDeque::new()) };
    pub static PENDING_POPUP_BROWSERS: RefCell<Vec<CreatedPopup>> = const { RefCell::new(Vec::new()) };
}

/// Called once by the compositor when its GPU state exists.
pub fn set_popup_factory(factory: PopupFactory) {
    POPUP_FACTORY.with_borrow_mut(|slot| *slot = Some(factory));
}

/// Oldest live awaiting setup, dropping ones CEF never created.
fn take_awaiting(now: Instant) -> Option<(i32, OsrRenderHandles)> {
    AWAITING_POPUPS.with_borrow_mut(|queue| {
        while let Some((at, opener, handles)) = queue.pop_front() {
            if now.saturating_duration_since(at) <= POPUP_CREATE_TIMEOUT {
                return Some((opener, handles));
            }
            log::warn!("popup from browser {opener} was never created; dropping its setup");
        }
        None
    })
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

fn popup_verdict(
    user_gesture: bool,
    url: &str,
    last: Option<Instant>,
    now: Instant,
) -> PopupVerdict {
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
            window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>,
            settings: Option<&mut BrowserSettings>,
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
                    let setup = POPUP_FACTORY.with_borrow(|factory| factory.as_ref().map(|make| make()));
                    match (setup, window_info, client, settings) {
                        (Some(setup), Some(window_info), Some(client), Some(settings)) => {
                            *window_info = setup.window_info;
                            *client = Some(setup.client);
                            *settings = setup.settings;
                            AWAITING_POPUPS.with_borrow_mut(|queue| {
                                queue.push_back((Instant::now(), opener_id, setup.handles))
                            });
                            return false as _; // let CEF create it, windowless
                        }
                        _ => PENDING_POPUPS.with_borrow_mut(|pending| pending.push((opener_id, url))),
                    }
                }
                verdict => {
                    log::info!("popup blocked ({verdict:?}) from browser {opener_id}: {url:?}");
                }
            }
            true as _
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else { return };
            if browser.is_popup() == 0 {
                return;
            }
            match take_awaiting(Instant::now()) {
                Some((opener_id, handles)) => PENDING_POPUP_BROWSERS.with_borrow_mut(|pending| {
                    pending.push(CreatedPopup {
                        opener_id,
                        browser: browser.clone(),
                        handles,
                    })
                }),
                None => {
                    log::warn!("popup browser {} created without a setup; closing", browser.identifier());
                    if let Some(host) = browser.host() {
                        host.close_browser(true as _);
                    }
                }
            }
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

    fn handles() -> OsrRenderHandles {
        OsrRenderHandles {
            size: std::rc::Rc::new(RefCell::new(winit::dpi::LogicalSize::new(1.0, 1.0))),
            texture: std::rc::Rc::new(RefCell::new(None)),
        }
    }

    #[test]
    fn awaiting_popups_match_in_order_and_expire() {
        let t0 = Instant::now();
        AWAITING_POPUPS.with_borrow_mut(|q| {
            q.clear();
            q.push_back((t0, 1, handles()));
            q.push_back((t0 + Duration::from_secs(10), 2, handles()));
            q.push_back((t0 + Duration::from_secs(11), 3, handles()));
        });
        // At t0+12s the first setup is stale (CEF dropped that popup): skip it.
        let now = t0 + Duration::from_secs(12);
        assert_eq!(take_awaiting(now).map(|(id, _)| id), Some(2));
        assert_eq!(take_awaiting(now).map(|(id, _)| id), Some(3));
        assert!(take_awaiting(now).is_none());
    }

    #[test]
    fn popup_url_scheme_filter() {
        assert!(popup_url_allowed(
            "https://accounts.example.test/o/oauth2/auth?x=1"
        ));
        assert!(popup_url_allowed("http://example.test/"));
        assert!(popup_url_allowed("HTTPS://example.test/"));
        assert!(!popup_url_allowed(""));
        assert!(!popup_url_allowed("about:blank"));
        assert!(!popup_url_allowed(
            "data:text/html,<script>alert(1)</script>"
        ));
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
        assert_eq!(
            popup_verdict(true, "data:,x", None, t0),
            PopupVerdict::BadScheme
        );
        assert_eq!(
            popup_verdict(true, url, Some(t0), t0),
            PopupVerdict::RateLimited
        );
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
