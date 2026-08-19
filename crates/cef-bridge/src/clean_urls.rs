// Strips well-known tracking query parameters (utm_*, fbclid, gclid,
// ...) from a navigation's URL before it loads — the same idea as
// uBlock Origin/ClearURLs' default rule set, just the common-case
// subset rather than their full regex-rule engine. Applied from
// `navigation.rs`'s `on_before_browse` (UI thread, same thread as the
// rest of that file — no cross-thread state needed here at all, unlike
// blocklist.rs) since it only ever needs to look at top-level
// navigations, not every sub-resource request, and rewriting via
// `Frame::load_url` needs a `Frame`, which `on_before_browse` has and
// blocklist.rs's IO-thread resource handler doesn't.
//
// Canceling a navigation and immediately loading the cleaned URL
// instead triggers `on_before_browse` again for that cleaned URL — not
// an infinite loop, since it has no tracking params left for `clean`
// to find, so the second call falls through to an ordinary allowed
// navigation.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Called once at startup (from the loaded settings.json) and again on
/// every Settings toggle.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Exact-match tracking params from the major ad/analytics/social
/// platforms — `utm_*` (Google Analytics campaign tagging) is handled
/// separately below via a prefix check rather than listed individually.
/// Deliberately pure ad-click/share-attribution IDs only — a param that
/// can double as an auth/identity token for the destination page itself
/// doesn't belong here even if it's *also* used for tracking. Two were
/// cut for exactly that reason: `mkt_tok` (Marketo) authenticates some
/// marketing-email unsubscribe/preference links, not just attributes
/// them; `oly_enc_id`/`oly_anon_id` (Olytics/Mather Economics) identify
/// a subscriber against a paywall on many news sites — the same risk
/// blocklist.rs's header comment already excludes `piano.io`/
/// `cxense.com` for.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid", "gclid", "gclsrc", "dclid", "msclkid", "twclid", "ttclid", "yclid", "igshid",
    "mc_eid", "mc_cid", "_hsenc", "_hsmi", "vero_id", "wickedid", "ncid", "spm", "scm", "si",
    "ref_src",
];

fn is_tracking_param(name: &str) -> bool {
    name.starts_with("utm_") || TRACKING_PARAMS.contains(&name)
}

/// `Some(cleaned)` only when at least one tracking param was actually
/// removed — callers treat `None` as "nothing to do, don't cancel the
/// navigation".
pub fn clean(url: &str) -> Option<String> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    let (before_fragment, fragment) = match url.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (url, None),
    };
    let (base, query) = before_fragment.split_once('?')?;
    let mut removed_any = false;
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            let name = pair.split('=').next().unwrap_or(pair);
            let tracking = is_tracking_param(name);
            removed_any |= tracking;
            !tracking
        })
        .collect();
    if !removed_any {
        return None;
    }
    let mut result = base.to_string();
    if !kept.is_empty() {
        result.push('?');
        result.push_str(&kept.join("&"));
    }
    if let Some(fragment) = fragment {
        result.push('#');
        result.push_str(fragment);
    }
    Some(result)
}
