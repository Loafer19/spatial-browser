// Host-suffix ad/tracker blocklist (Peter Lowe list via include_str!;
// excludes piano.io/cxense.com — paywall infra). Custom hosts come from Settings.
// Must use Sync statics, not thread_local: CEF calls resource handlers on the IO
// thread; UI-thread writes would never be visible otherwise.

use cef::{self, rc::Rc, *};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

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

/// Startup + every Settings add/remove of custom blocked hosts.
pub fn set_custom_hosts(hosts: Vec<String>) {
    if let Ok(mut current) = CUSTOM_HOSTS.lock() {
        *current = hosts;
    }
}

const BLOCKED_DOMAINS_TXT: &str = include_str!("blocked_domains.txt");

/// Suffix set for O(labels) host matching against the compiled-in list.
static BLOCKED_DOMAINS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    BLOCKED_DOMAINS_TXT
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
});

/// Strip scheme and path/query/fragment/port (mirrors compositor `host_of`).
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

/// `a.b.example.com` → `b.example.com` → `example.com` → `com`.
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
    // Poisoned lock → treat as no custom hosts; don't panic CEF's IO thread.
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
