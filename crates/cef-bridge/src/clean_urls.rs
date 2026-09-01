// Strip common tracking query params (utm_*, fbclid, …) on top-level
// navigations via on_before_browse. Re-entry after load_url is fine: cleaned
// URLs have nothing left to strip. Omit params that double as auth tokens
// (mkt_tok, oly_*).

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Startup + Settings toggle.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Attribution IDs only (`utm_*` handled via prefix). Not auth/identity tokens.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid", "gclid", "gclsrc", "dclid", "msclkid", "twclid", "ttclid", "yclid", "igshid",
    "mc_eid", "mc_cid", "_hsenc", "_hsmi", "vero_id", "wickedid", "ncid", "spm", "scm", "si",
    "ref_src",
];

fn is_tracking_param(name: &str) -> bool {
    name.starts_with("utm_") || TRACKING_PARAMS.contains(&name)
}

/// `Some(cleaned)` if any tracking param was removed; `None` = leave alone.
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
