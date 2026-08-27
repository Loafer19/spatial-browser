// Ad/tracker request blocking: a static list of well-known ad-serving
// and tracking domains, matched by suffix against every resource
// request's host (not just top-level navigations) and canceled if it
// matches. Deliberately not a full filter-rule engine (EasyList/
// uBlock-style syntax, cosmetic hiding of the empty space an ad would
// have occupied) — that's a project of its own. A domain blocklist
// blocks noticeably less, but costs near nothing to maintain.
//
// blocked_domains.txt is Peter Lowe's ad-server list
// (https://pgl.yoyo.org/adservers/, `hostformat=nohtml`) — ~3500
// domains, focused specifically on ads/trackers rather than the
// broader malware/gambling/etc. categories a general "unified hosts"
// list would pull in. Two exclusions from the upstream list: piano.io
// and cxense.com, both Piano's metered-paywall infrastructure on many
// news sites (confirmed showing up in a live test against CNN) —
// blocking those risks breaking paid-article access, not just making a
// page less tracked. Compiled in via `include_str!`, not downloaded at
// runtime: no network dependency, works offline, and a page load can't
// race an in-progress fetch. Re-running the same fetch+filter and
// replacing this file is how to pick up upstream's updates; there's no
// automation for that here, deliberately — it changes rarely enough
// that a manual refresh is simpler than maintaining a fetcher.
//
// The compiled-in list is deliberately not user-editable (that's what
// compositor::persistence::settings::AppSettings::custom_blocked_hosts
// is for, applied on top of this one via `set_enabled`/
// `set_custom_hosts` below) — it's the "just works, don't think about
// it" baseline; the settings page's editable list is for whatever that
// baseline misses.

use cef::{self, rc::Rc, *};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

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
static PETER_LOWE_ENABLED: AtomicBool = AtomicBool::new(true);
static CUSTOM_HOSTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Master content-filtering switch (Settings → Blocking).
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether the compiled-in Peter Lowe host list participates.
pub fn set_peter_lowe_enabled(enabled: bool) {
    PETER_LOWE_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Called once at startup and again on every add/remove in the settings
/// page's blocked-hosts list.
pub fn set_custom_hosts(hosts: Vec<String>) {
    if let Ok(mut current) = CUSTOM_HOSTS.lock() {
        *current = hosts;
    }
}

/// The raw list, one domain per line (see this file's header comment
/// for provenance) — parsed once into `BLOCKED_DOMAINS` below, not
/// re-split on every request.
const BLOCKED_DOMAINS_TXT: &str = include_str!("blocked_domains.txt");

/// A set, not the plain list: `is_blocked` below checks each of a
/// host's own progressively-shorter suffixes against this (`a.b.com`,
/// `b.com`, `com`) for an O(labels-in-the-host) lookup, equivalent to
/// "host == domain or ends with `.`+domain for any of ~3500 domains"
/// without actually scanning all ~3500 of them per request.
static BLOCKED_DOMAINS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    BLOCKED_DOMAINS_TXT
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
});

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

/// `host` itself, then with its leftmost label stripped repeatedly
/// (`a.b.example.com` → `b.example.com` → `example.com` → `com`) — the
/// suffixes that "does any domain in the (large) blocklist match host,
/// exactly or as a parent domain" reduces to checking membership of,
/// one `HashSet` lookup each, rather than testing host against every
/// entry in the set.
fn suffixes(host: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(host), |s| s.split_once('.').map(|(_, rest)| rest))
}

fn is_blocked(host: &str) -> bool {
    if !ENABLED.load(Ordering::Relaxed) {
        return false;
    }
    if PETER_LOWE_ENABLED.load(Ordering::Relaxed)
        && suffixes(host).any(|suffix| BLOCKED_DOMAINS.contains(suffix))
    {
        return true;
    }
    // A poisoned lock (a panic while holding it, on either thread) is
    // treated as "no custom hosts" rather than propagating the panic
    // into CEF's IO thread — losing the custom list until the next
    // `set_custom_hosts` is a much smaller problem than taking the
    // whole browser process down. The custom list is small (user-added
    // one at a time via Settings), so a plain per-domain suffix check
    // here is fine — no need for the same set-of-suffixes trick as the
    // ~3500-entry compiled-in list above.
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
            if !ENABLED.load(Ordering::Relaxed) {
                return ReturnValue::CONTINUE;
            }
            let referrer = cef::CefString::from(&request.referrer_url()).to_string();
            let rtype = request.resource_type();
            if crate::filter_engine::check_request(&url, &referrer, rtype) {
                return ReturnValue::CANCEL;
            }
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
