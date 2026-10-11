// Origin normalization and autofill eligibility.

/// Strip path/query; keep scheme + host (+ non-default port).
pub fn normalize_origin(url_or_origin: &str) -> String {
    let s = url_or_origin.trim();
    let is_scheme = |sch: &str| {
        let mut chars = sch.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    };
    let (scheme, after) = match s.split_once("://") {
        Some((sch, rest)) if is_scheme(sch) => (sch.to_ascii_lowercase(), rest),
        // `data:text/html,https://x` and the like: not an origin.
        Some(_) => return String::new(),
        None => {
            // `data:`, `javascript:`, `about:`, `mailto:` … are not web
            // origins; a bare `host:port` (digits after the colon) is.
            if let Some((head, tail)) = s.split_once(':') {
                let port = tail.split(['/', '?', '#']).next().unwrap_or("");
                if is_scheme(head)
                    && !(port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty())
                {
                    return String::new();
                }
            }
            ("https".to_string(), s)
        }
    };
    let hostport = after.split(['/', '?', '#']).next().unwrap_or(after);
    if hostport.is_empty() {
        return String::new();
    }
    format!("{scheme}://{}", hostport.to_ascii_lowercase())
}

/// Whether a page origin may get autofill: HTTPS anywhere, plain HTTP only
/// for this machine and the local network (routers, NAS, dev servers), where
/// there is no CA to buy a certificate from. Public HTTP is MITM-able.
pub fn is_autofill_origin(origin: &str) -> bool {
    // Needs an explicit scheme: normalize_origin reads `data:...` as a bare
    // host and would call it https.
    if !origin.trim_start().contains("://") {
        return false;
    }
    let origin = normalize_origin(origin);
    let Some((scheme, hostport)) = origin.split_once("://") else {
        return false;
    };
    match scheme {
        "https" => true,
        "http" => is_local_host(host_of(hostport)),
        _ => false,
    }
}

/// Host part of `host[:port]`, `[v6]:port` or `user@host`.
pub(super) fn host_of(hostport: &str) -> &str {
    let hostport = hostport.rsplit_once('@').map_or(hostport, |(_, h)| h);
    if let Some(rest) = hostport.strip_prefix('[') {
        return rest.split_once(']').map_or(rest, |(h, _)| h);
    }
    hostport.split_once(':').map_or(hostport, |(h, _)| h)
}

pub(super) fn is_local_host(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => {
            // ::1, fc00::/7 unique-local, fe80::/10 link-local.
            ip.is_loopback()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
        Err(_) => false,
    }
}

/// Match key for autofill: hostname without leading `www.`, ignoring scheme.
/// So `https://www.example.com` and `http://example.com` share a vault entry.
pub fn host_key(url_or_origin: &str) -> String {
    let origin = normalize_origin(url_or_origin);
    let hostport = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin.as_str());
    hostport
        .strip_prefix("www.")
        .unwrap_or(hostport)
        .to_ascii_lowercase()
}
