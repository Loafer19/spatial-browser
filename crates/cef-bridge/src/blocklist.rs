// Ad/tracker request blocking: a static list of well-known ad-serving
// and tracking domains, matched by suffix against every resource
// request's host (not just top-level navigations) and canceled if it
// matches. Deliberately not a full filter-rule engine (EasyList/
// uBlock-style syntax, cosmetic hiding of the empty space an ad would
// have occupied) — that's a project of its own. A domain blocklist
// blocks noticeably less, but costs near nothing to maintain: adding a
// domain is one line, and it needs no rule-syntax parser at all.
//
// The compiled-in list below is deliberately not user-editable (that's
// what compositor::persistence::settings::AppSettings::
// custom_blocked_hosts is for, applied on top of this one via
// `set_enabled`/`set_custom_hosts` below) — it's the "just works, don't
// think about it" baseline; the settings page's editable list is for
// whatever that baseline misses.

use cef::{self, rc::Rc, *};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

// Real shared statics, not `thread_local!`: CEF calls
// `ImplRequestHandler::resource_request_handler` and
// `ImplResourceRequestHandler::on_before_resource_load` on its IO
// thread, a different OS thread from the one the rest of this app runs
// on (winit's event loop, where a settings change actually happens) —
// unlike `on_before_browse` and everything else in navigation.rs, which
// CEF calls on the UI thread, matching this app's own thread, which is
// why those *can* get away with `thread_local!`. A `thread_local!` here
// would silently read back its own thread's default (empty/enabled)
// state forever, no matter what the UI thread wrote — confirmed
// empirically: the compiled-in `BLOCKED_DOMAINS` list (a plain const,
// unaffected either way) blocked real requests, but a custom host added
// through the settings page never took effect.
static ENABLED: AtomicBool = AtomicBool::new(true);
static CUSTOM_HOSTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Called once at startup (from the loaded settings.json) and again on
/// every Ctrl+, toggle.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Called once at startup and again on every add/remove in the settings
/// page's blocked-hosts list.
pub fn set_custom_hosts(hosts: Vec<String>) {
    if let Ok(mut current) = CUSTOM_HOSTS.lock() {
        *current = hosts;
    }
}

/// Well-known ad-serving/tracking domains, matched by suffix (`host ==
/// domain` or `host` ends with `.{domain}`) — covers most of what a
/// site actually loads ads/trackers *through*, without trying to be
/// exhaustive. Extend by appending a line.
const BLOCKED_DOMAINS: &[&str] = &[
    // Google's ad stack
    "doubleclick.net",
    "2mdn.net",
    "googlesyndication.com",
    "googleadservices.com",
    "googletagmanager.com",
    "googletagservices.com",
    "google-analytics.com",
    "adservice.google.com",
    // Other major ad networks / exchanges
    "amazon-adsystem.com",
    "adnxs.com",
    "adsrvr.org",
    "criteo.com",
    "criteo.net",
    "taboola.com",
    "outbrain.com",
    "scorecardresearch.com",
    "quantserve.com",
    "quantcount.com",
    "moatads.com",
    "adform.net",
    "rubiconproject.com",
    "pubmatic.com",
    "openx.net",
    "casalemedia.com",
    "indexexchange.com",
    "media.net",
    "adroll.com",
    "serving-sys.com",
    "smartadserver.com",
    "yieldmo.com",
    "sharethrough.com",
    "triplelift.com",
    "sovrn.com",
    "gumgum.com",
    "teads.tv",
    "mgid.com",
    "revcontent.com",
    // Social widgets' tracking pixels
    "connect.facebook.net",
    "bat.bing.com",
    "ads-twitter.com",
    // Session-replay / behavior analytics
    "hotjar.com",
    "fullstory.com",
    "mouseflow.com",
    // Confirmed live against a real ad-heavy site (CNN) — every one of
    // these fired a real ad/tracker request that the list above missed.
    // Deliberately not adding tinypass.com/piano.io/cxense.com even
    // though they showed up in the same test: those are Piano's
    // metered-paywall infrastructure on many news sites, not just ads —
    // blocking them risks breaking paid-article access, not just making
    // a page less tracked.
    "permutive.app",
    "permutive.com",
    "chartbeat.com",
    "chartbeat.net",
    "imrworldwide.com",
    "demdex.net",
    "zetaglobal.net",
    "indexww.com",
    "adsafeprotected.com",
    "rezync.com",
    "ad-delivery.net",
    "stickyadstv.com",
    "boomtrain.com",
    "bounceexchange.com",
    "btloader.com",
];

/// Same host-extraction logic as `compositor::persistence::bookmarks::
/// host_of` (duplicated rather than shared: cef-bridge doesn't, and
/// shouldn't, depend on the compositor crate) — strips the scheme and
/// everything from the first `/`, `?`, `#`, or `:` (port) onward.
fn host_of(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    after_scheme
        .split(['/', '?', '#', ':'])
        .next()
        .unwrap_or(after_scheme)
}

fn matches_domain(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn is_blocked(host: &str) -> bool {
    if !ENABLED.load(Ordering::Relaxed) {
        return false;
    }
    if BLOCKED_DOMAINS
        .iter()
        .any(|domain| matches_domain(host, domain))
    {
        return true;
    }
    // A poisoned lock (a panic while holding it, on either thread) is
    // treated as "no custom hosts" rather than propagating the panic
    // into CEF's IO thread — losing the custom list until the next
    // `set_custom_hosts` is a much smaller problem than taking the
    // whole browser process down.
    CUSTOM_HOSTS
        .lock()
        .map(|hosts| hosts.iter().any(|domain| matches_domain(host, domain)))
        .unwrap_or(false)
}

#[derive(Clone)]
pub struct OsrResourceRequestHandler {}

wrap_resource_request_handler! {
    pub struct ResourceRequestHandlerBuilder {
        handler: OsrResourceRequestHandler,
    }

    impl ResourceRequestHandler {
        fn on_before_resource_load(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            let Some(request) = request else {
                return ReturnValue::CONTINUE;
            };
            let url = cef::CefString::from(&request.url()).to_string();
            if is_blocked(host_of(&url)) {
                return ReturnValue::CANCEL;
            }
            ReturnValue::CONTINUE
        }
    }
}

impl ResourceRequestHandlerBuilder {
    pub fn build(handler: OsrResourceRequestHandler) -> cef::ResourceRequestHandler {
        Self::new(handler)
    }
}
