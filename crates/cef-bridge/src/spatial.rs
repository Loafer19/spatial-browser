// `spatial://page/<kind>/<token>`: built-in pages (bookmarks, settings,
// passwords, …) served from memory instead of `data:` URLs.
//
// The compositor builds each page's HTML, `publish`es it under a random
// 128-bit token, and loads the URL. The token is the capability: a web page
// cannot guess it, and once a browser loads it (`bind`) only that browser is
// served it and only that browser's actions count as internal
// (`issued_to`). Refreshing a list swaps the HTML under the tile's token
// (`adopt`) and reloads the same browser — no new CEF browser per click.
//
// Scheme flags: STANDARD (real origin, `location.search`, sessionStorage),
// SECURE, LOCAL (file:-like — web pages can neither link to, navigate to nor
// iframe it). Not CORS/fetch-enabled, not CSP-bypassing. On top of that,
// `navigation_allowed` refuses spatial:// navigations a web page starts and
// the handler factory serves main-frame requests only.

use cef::{self, rc::Rc, *};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

pub const SCHEME: &str = "spatial";
const PAGE_HOST: &str = "page";
pub const PAGE_URL_PREFIX: &str = "spatial://page/";
const TOKEN_HEX_LEN: usize = 32;
const MAX_KIND_LEN: usize = 32;
/// Published but not yet loaded by any browser. Every compositor path binds
/// right after publishing, so this is only a backstop against leaks (and
/// roomy enough for the compositor's parallel tests). Oldest beyond it go.
const MAX_UNBOUND: usize = 64;

/// No network access at all: inline script/style, images only as data:
/// URIs (favicon tiles come from the local cache).
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; \
     script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
     img-src data:; base-uri 'none'; frame-ancestors 'none'";

/// Page HTML, wiped when the last holder (registry or in-flight response)
/// drops it — the passwords page carries usernames and generated passwords.
pub struct PageBody(Vec<u8>);

impl PageBody {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for PageBody {
    fn drop(&mut self) {
        for byte in self.0.iter_mut() {
            // Volatile so the optimizer cannot drop a write to dying memory.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

fn valid_kind(kind: &str) -> bool {
    !kind.is_empty()
        && kind.len() <= MAX_KIND_LEN
        && kind.starts_with(|c: char| c.is_ascii_lowercase())
        && kind
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_token(token: &str) -> bool {
    token.len() == TOKEN_HEX_LEN
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `spatial://page/<kind>/<token>`; `None` for a malformed kind or token.
pub fn page_url(kind: &str, token: &str) -> Option<String> {
    (valid_kind(kind) && valid_token(token)).then(|| format!("{PAGE_URL_PREFIX}{kind}/{token}"))
}

/// `(kind, token)` of a built-in page URL. A query or fragment is ignored;
/// anything else (extra segments, bad token, other host) is rejected.
pub fn parse_page_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix(PAGE_URL_PREFIX)?;
    let path = rest.split(['?', '#']).next().unwrap_or(rest);
    let (kind, token) = path.split_once('/')?;
    (valid_kind(kind) && valid_token(token)).then_some((kind, token))
}

/// Any `spatial:` URL (case-insensitive scheme), built-in page or not.
pub fn is_spatial_url(url: &str) -> bool {
    url.split_once(':')
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case(SCHEME))
}

/// May a navigation to a `spatial:` URL proceed? Only in the main frame, and
/// only when the compositor started it (CreateBrowser / LoadURL set
/// `direct_load`) or the frame is new or already on a built-in page (reload,
/// back/forward). A web page's `location = 'spatial://…'` or iframe is refused.
pub fn navigation_allowed(is_main_frame: bool, direct_load: bool, current_url: &str) -> bool {
    is_main_frame
        && (direct_load
            || current_url.is_empty()
            || current_url == "about:blank"
            || current_url.starts_with(PAGE_URL_PREFIX))
}

struct Entry {
    kind: String,
    body: Arc<PageBody>,
    owner: Option<i32>,
    seq: u64,
}

#[derive(Default)]
struct Registry {
    entries: HashMap<String, Entry>,
    next_seq: u64,
}

impl Registry {
    fn publish(&mut self, kind: &str, token: &str, html: String) -> Option<String> {
        let url = page_url(kind, token)?;
        self.next_seq += 1;
        self.entries.insert(
            token.to_string(),
            Entry {
                kind: kind.to_string(),
                body: Arc::new(PageBody(html.into_bytes())),
                owner: None,
                seq: self.next_seq,
            },
        );
        self.evict_unbound();
        Some(url)
    }

    fn evict_unbound(&mut self) {
        let mut unbound: Vec<(u64, String)> = self
            .entries
            .iter()
            .filter(|(_, e)| e.owner.is_none())
            .map(|(t, e)| (e.seq, t.clone()))
            .collect();
        if unbound.len() <= MAX_UNBOUND {
            return;
        }
        unbound.sort_unstable();
        let excess = unbound.len() - MAX_UNBOUND;
        for (_, token) in unbound.into_iter().take(excess) {
            self.entries.remove(&token);
        }
    }

    /// Give `url`'s token to `browser_id`; the browser's previous token (if
    /// any) is dropped. Refuses a token another browser already holds.
    fn bind(&mut self, url: &str, browser_id: i32) -> bool {
        let Some((kind, token)) = parse_page_url(url) else {
            return false;
        };
        match self.entries.get(token) {
            Some(e) if e.kind == kind && e.owner.is_none_or(|o| o == browser_id) => {}
            _ => return false,
        }
        self.entries
            .retain(|t, e| t == token || e.owner != Some(browser_id));
        if let Some(e) = self.entries.get_mut(token) {
            e.owner = Some(browser_id);
        }
        true
    }

    /// Move freshly published `new_url` content under the token
    /// `browser_id` already holds, so a refresh reloads the same URL (no
    /// history growth, actions stay trusted). Without a held token,
    /// `new_url` is bound as is. Returns the URL to load.
    fn adopt(&mut self, browser_id: i32, new_url: &str) -> Option<String> {
        let (kind, new_token) = parse_page_url(new_url)?;
        if self.entries.get(new_token)?.owner.is_some() {
            return None;
        }
        let held = self
            .entries
            .iter()
            .find(|(_, e)| e.owner == Some(browser_id))
            .map(|(t, _)| t.clone());
        let Some(held) = held else {
            return self.bind(new_url, browser_id).then(|| new_url.to_string());
        };
        let fresh = self.entries.remove(new_token)?;
        let entry = self.entries.get_mut(&held)?;
        entry.kind = fresh.kind;
        entry.body = fresh.body; // old body wiped on drop
        page_url(kind, &held)
    }

    fn issued_to(&self, url: &str, browser_id: i32) -> bool {
        parse_page_url(url).is_some_and(|(kind, token)| {
            self.entries
                .get(token)
                .is_some_and(|e| e.kind == kind && e.owner == Some(browser_id))
        })
    }

    /// Content for a load of `url` by `requester`: an unbound token (first
    /// load, before `bind`) or one bound to that browser.
    fn body(&self, url: &str, requester: Option<i32>) -> Option<Arc<PageBody>> {
        let (kind, token) = parse_page_url(url)?;
        let e = self.entries.get(token)?;
        (e.kind == kind && (e.owner.is_none() || e.owner == requester)).then(|| e.body.clone())
    }

    fn forget_browser(&mut self, browser_id: i32) {
        self.entries.retain(|_, e| e.owner != Some(browser_id));
    }

    fn remove_kind(&mut self, kind: &str) {
        self.entries.retain(|_, e| e.kind != kind);
    }
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(Default::default);

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Store `html` under `token` (caller-generated, 32 lowercase hex chars from
/// a CSPRNG) and return its URL; `None` if `kind` or `token` is malformed.
pub fn publish(kind: &str, token: &str, html: String) -> Option<String> {
    registry().publish(kind, token, html)
}

/// Bind `url`'s token to the browser that just loaded it.
pub fn bind(url: &str, browser_id: i32) -> bool {
    registry().bind(url, browser_id)
}

/// Refresh-in-place: see `Registry::adopt`.
pub fn adopt(browser_id: i32, new_url: &str) -> Option<String> {
    registry().adopt(browser_id, new_url)
}

/// Whether `url` is a built-in page whose token was issued to `browser_id`.
pub fn issued_to(url: &str, browser_id: i32) -> bool {
    registry().issued_to(url, browser_id)
}

/// The HTML a load of `url` by `requester` would get (`None` → 404).
pub fn body(url: &str, requester: Option<i32>) -> Option<Arc<PageBody>> {
    registry().body(url, requester)
}

/// Drop (and wipe) a closed browser's page.
pub fn forget_browser(browser_id: i32) {
    registry().forget_browser(browser_id);
}

/// Drop (and wipe) every page of `kind`, bound or not.
pub fn remove_kind(kind: &str) {
    registry().remove_kind(kind);
}

/// `App::on_register_custom_schemes` — runs in every process.
pub(crate) fn register_scheme(registrar: &mut SchemeRegistrar) {
    let options = SchemeOptions::STANDARD.get_raw()
        | SchemeOptions::SECURE.get_raw()
        | SchemeOptions::LOCAL.get_raw();
    if registrar.add_custom_scheme(Some(&SCHEME.into()), options as _) == 0 {
        log::error!("failed to register the {SCHEME}:// scheme");
    }
}

/// After context init (browser process): serve `spatial://page/…`.
pub(crate) fn register_handler_factory() {
    let mut factory = SpatialSchemeHandlerFactory::new();
    if register_scheme_handler_factory(
        Some(&SCHEME.into()),
        Some(&PAGE_HOST.into()),
        Some(&mut factory),
    ) == 0
    {
        log::error!("failed to register the {SCHEME}:// handler factory");
    }
}

wrap_scheme_handler_factory! {
    struct SpatialSchemeHandlerFactory {}

    impl SchemeHandlerFactory {
        fn create(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            let (url, main_frame) = request
                .map(|r| {
                    (
                        CefString::from(&r.url()).to_string(),
                        r.resource_type() == ResourceType::MAIN_FRAME,
                    )
                })
                .unwrap_or_default();
            let requester = browser.map(|b| b.identifier());
            // Subframes and subresources never get a page, whoever asks.
            let body = if main_frame { body(&url, requester) } else { None };
            if body.is_none() {
                let kind = parse_page_url(&url).map_or("?", |(kind, _)| kind);
                log::warn!(
                    "{SCHEME}:// 404 for {kind} (browser {requester:?}, main frame {main_frame})"
                );
            }
            Some(PageResourceHandler::new(Arc::new(Mutex::new(Served {
                body,
                offset: 0,
            }))))
        }
    }
}

struct Served {
    body: Option<Arc<PageBody>>,
    offset: usize,
}

fn lock_served(served: &Mutex<Served>) -> MutexGuard<'_, Served> {
    served
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

wrap_resource_handler! {
    struct PageResourceHandler {
        served: Arc<Mutex<Served>>,
    }

    impl ResourceHandler {
        fn open(
            &self,
            _request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            // Answer synchronously; the body is already in memory.
            if let Some(handle_request) = handle_request {
                *handle_request = true as _;
            }
            true as _
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let served = lock_served(&self.served);
            let len = served.body.as_ref().map_or(0, |b| b.as_bytes().len());
            if let Some(response) = response {
                let (status, text) = if served.body.is_some() {
                    (200, "OK")
                } else {
                    (404, "Not Found")
                };
                response.set_status(status);
                response.set_status_text(Some(&text.into()));
                response.set_mime_type(Some(&"text/html".into()));
                response.set_charset(Some(&"utf-8".into()));
                for (name, value) in [
                    ("Cache-Control", "no-store"),
                    ("Content-Security-Policy", CONTENT_SECURITY_POLICY),
                    ("X-Content-Type-Options", "nosniff"),
                    ("Referrer-Policy", "no-referrer"),
                ] {
                    response.set_header_by_name(Some(&name.into()), Some(&value.into()), true as _);
                }
            }
            if let Some(response_length) = response_length {
                *response_length = len as i64;
            }
        }

        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let mut served = lock_served(&self.served);
            let offset = served.offset;
            let remaining = served
                .body
                .as_ref()
                .map_or(&[][..], |b| b.as_bytes().get(offset..).unwrap_or_default());
            let n = remaining.len().min(bytes_to_read.max(0) as usize);
            if n == 0 || data_out.is_null() {
                if let Some(bytes_read) = bytes_read {
                    *bytes_read = 0;
                }
                return false as _; // complete
            }
            // SAFETY: CEF hands a writable buffer of `bytes_to_read` bytes;
            // `n` is at most that and at most what is left of the body.
            unsafe { std::ptr::copy_nonoverlapping(remaining.as_ptr(), data_out, n) };
            served.offset += n;
            if let Some(bytes_read) = bytes_read {
                *bytes_read = n as _;
            }
            true as _
        }

        fn cancel(&self) {
            lock_served(&self.served).body = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T1: &str = "0123456789abcdef0123456789abcdef";
    const T2: &str = "fedcba9876543210fedcba9876543210";
    const T3: &str = "00000000000000000000000000000003";

    fn token(n: usize) -> String {
        format!("{n:032x}")
    }

    #[test]
    fn url_round_trip_and_malformed_rejected() {
        let url = page_url("vault-list", T1).unwrap();
        assert_eq!(url, format!("spatial://page/vault-list/{T1}"));
        assert_eq!(parse_page_url(&url), Some(("vault-list", T1)));
        assert_eq!(
            parse_page_url(&format!("{url}?x=1#frag")),
            Some(("vault-list", T1))
        );
        for bad in [
            "spatial://page/vault-list".to_string(),
            "spatial://page/".to_string(),
            format!("spatial://page//{T1}"),
            format!("spatial://page/Bookmarks/{T1}"),
            format!("spatial://page/1abc/{T1}"),
            format!("spatial://page/bookmarks/{T1}/extra"),
            format!("spatial://page/bookmarks/{}", T1.to_uppercase()),
            format!("spatial://page/bookmarks/{}", &T1[1..]),
            format!("spatial://page/bookmarks/{T1}0"),
            format!("spatial://page/bookmarks/{}g", &T1[1..]),
            format!("spatial://other/bookmarks/{T1}"),
            format!("https://page/bookmarks/{T1}"),
            format!("data:text/html,spatial://page/bookmarks/{T1}"),
        ] {
            assert_eq!(parse_page_url(&bad), None, "{bad}");
        }
        assert_eq!(page_url("Bad Kind", T1), None);
        assert_eq!(page_url("help", "nothex"), None);
    }

    #[test]
    fn scheme_detection_is_case_insensitive() {
        assert!(is_spatial_url("spatial://page/x"));
        assert!(is_spatial_url("SPATIAL://page/x"));
        assert!(!is_spatial_url("https://spatial.test/"));
        assert!(!is_spatial_url("spatialx://page"));
    }

    #[test]
    fn navigation_from_web_pages_is_refused() {
        let own = format!("spatial://page/help/{T1}");
        // Compositor-started: new tile, refresh, or LoadURL anywhere.
        assert!(navigation_allowed(true, true, "https://a.test/"));
        assert!(navigation_allowed(true, false, ""));
        assert!(navigation_allowed(true, false, "about:blank"));
        assert!(navigation_allowed(true, false, &own));
        // A web page's `location = …`, link or iframe.
        assert!(!navigation_allowed(true, false, "https://evil.test/"));
        assert!(!navigation_allowed(true, false, "data:text/html,x"));
        assert!(!navigation_allowed(false, true, &own));
        assert!(!navigation_allowed(false, false, &own));
    }

    #[test]
    fn publish_get_and_unknown_token_404() {
        let mut r = Registry::default();
        let url = r.publish("help", T1, "<p>hi</p>".into()).unwrap();
        assert_eq!(r.body(&url, None).unwrap().as_bytes(), b"<p>hi</p>");
        assert_eq!(r.body(&url, Some(7)).unwrap().as_bytes(), b"<p>hi</p>");
        // Unknown token, wrong kind, malformed URL: 404.
        assert!(r.body(&format!("spatial://page/help/{T2}"), None).is_none());
        assert!(r
            .body(&format!("spatial://page/settings/{T1}"), None)
            .is_none());
        assert!(r.body("spatial://page/help/nope", None).is_none());
        assert!(r.publish("help", "short", String::new()).is_none());
    }

    #[test]
    fn bound_page_is_served_and_trusted_only_for_its_browser() {
        let mut r = Registry::default();
        let url = r.publish("bookmarks", T1, "x".into()).unwrap();
        assert!(!r.issued_to(&url, 1), "unbound is not trusted");
        assert!(r.bind(&url, 1));
        assert!(r.issued_to(&url, 1));
        assert!(!r.issued_to(&url, 2));
        assert!(r.body(&url, Some(1)).is_some());
        assert!(r.body(&url, Some(2)).is_none());
        assert!(r.body(&url, None).is_none());
        // Another browser cannot take it over.
        assert!(!r.bind(&url, 2));
        // Kind is part of the capability.
        assert!(!r.issued_to(&format!("spatial://page/vault-list/{T1}"), 1));
    }

    #[test]
    fn bind_drops_the_browsers_previous_page() {
        let mut r = Registry::default();
        let first = r.publish("help", T1, "1".into()).unwrap();
        let second = r.publish("help", T2, "2".into()).unwrap();
        assert!(r.bind(&first, 5));
        assert!(r.bind(&second, 5));
        assert!(r.body(&first, Some(5)).is_none());
        assert!(r.issued_to(&second, 5));
    }

    #[test]
    fn adopt_keeps_the_tiles_token() {
        let mut r = Registry::default();
        let open = r.publish("vault-unlock", T1, "unlock".into()).unwrap();
        assert!(r.bind(&open, 3));
        let fresh = r.publish("vault-list", T2, "list".into()).unwrap();
        let url = r.adopt(3, &fresh).unwrap();
        assert_eq!(url, format!("spatial://page/vault-list/{T1}"));
        assert_eq!(r.body(&url, Some(3)).unwrap().as_bytes(), b"list");
        assert!(r.issued_to(&url, 3));
        assert!(!r.issued_to(&open, 3), "old kind no longer valid");
        assert!(r.body(&fresh, None).is_none(), "fresh token consumed");
        assert_eq!(r.entries.len(), 1);
        // A browser without a page binds the fresh URL as is.
        let other = r.publish("help", T3, "h".into()).unwrap();
        assert_eq!(r.adopt(9, &other).as_deref(), Some(other.as_str()));
        assert!(r.issued_to(&other, 9));
        // Cannot adopt someone else's bound page.
        assert_eq!(r.adopt(3, &other), None);
    }

    #[test]
    fn close_and_lock_evict() {
        let mut r = Registry::default();
        let a = r.publish("vault-list", T1, "a".into()).unwrap();
        let b = r.publish("vault-list", T2, "b".into()).unwrap();
        let c = r.publish("help", T3, "c".into()).unwrap();
        assert!(r.bind(&a, 1) && r.bind(&c, 2));
        r.forget_browser(1);
        assert!(r.body(&a, Some(1)).is_none());
        assert!(r.body(&b, None).is_some());
        r.remove_kind("vault-list");
        assert!(r.body(&b, None).is_none());
        assert!(r.issued_to(&c, 2));
    }

    #[test]
    fn unbound_pages_are_capped_oldest_first() {
        let mut r = Registry::default();
        let bound = r.publish("help", &token(1000), "b".into()).unwrap();
        assert!(r.bind(&bound, 1));
        let urls: Vec<String> = (0..MAX_UNBOUND + 3)
            .map(|n| r.publish("help", &token(n), n.to_string()).unwrap())
            .collect();
        assert_eq!(r.entries.len(), MAX_UNBOUND + 1);
        for url in &urls[..3] {
            assert!(r.body(url, None).is_none(), "{url} should be evicted");
        }
        assert!(r.body(&urls[3], None).is_some());
        assert!(
            r.body(&bound, Some(1)).is_some(),
            "bound pages are never evicted"
        );
    }

    #[test]
    fn global_registry_round_trip() {
        let t = token(0xfeed_0001);
        let url = publish("palette", &t, "s".into()).unwrap();
        assert!(bind(&url, 424_242));
        assert!(issued_to(&url, 424_242));
        forget_browser(424_242);
        assert!(!issued_to(&url, 424_242));
        assert!(body(&url, None).is_none());
    }
}
