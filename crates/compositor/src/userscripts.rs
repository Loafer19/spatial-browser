// A Tampermonkey/Greasemonkey-style userscript runner — not real Chrome
// extensions (CEF's Alloy/windowless bootstrap exposes no extension-
// loading API at all, confirmed empirically: zero of ~5900 bound
// functions touch it), just the one slice of what extensions are
// commonly used for that's already exactly what this codebase's own
// clipboard_bridge.rs/blocklist.rs do by hand: inject JS into pages
// whose URL matches a pattern.
//
// Each `.js` file under `~/.config/spatial-browser/userscripts/` is one
// script. Anywhere in the file, a line `// @match <pattern>` registers
// a URL match pattern for it (any number of `@match` lines — the script
// runs if the page's URL matches *any* of them); a script with none is
// skipped entirely, since it could never run. `<pattern>` is a plain
// wildcard glob against the full URL (`*` matches anything, everything
// else literal) — not the full WebExtension match-pattern spec
// (scheme/host/path parsed separately), which is more precision than a
// personal userscript file actually needs.
//
//   // @match *://*.reddit.com/*
//   (function() {
//     document.body.style.filter = 'grayscale(1)';
//   })();
//
// Loaded once at startup — editing a script needs a restart to take
// effect, not a live-reload mechanism, matching how rarely this
// actually changes.

use std::path::PathBuf;

pub struct UserScript {
    pub matches: Vec<String>,
    pub code: String,
}

fn dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/userscripts")
}

/// Reads every `.js` file in the userscripts directory. Missing
/// directory or unreadable/matchless files are skipped, not errors —
/// there's nothing a user needs to fix for the common case of simply
/// not having any userscripts yet.
pub fn load() -> Vec<UserScript> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut scripts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("js") {
            continue;
        }
        let Ok(code) = std::fs::read_to_string(&path) else {
            log::warn!("userscripts: couldn't read {path:?}");
            continue;
        };
        let matches: Vec<String> = code
            .lines()
            .filter_map(|line| line.trim().strip_prefix("// @match "))
            .map(|pattern| pattern.trim().to_string())
            .collect();
        if matches.is_empty() {
            log::warn!("userscripts: {path:?} has no `// @match` lines, skipping");
            continue;
        }
        scripts.push(UserScript { matches, code });
    }
    scripts
}

/// A plain wildcard glob (`*` = anything, everything else literal)
/// against the full URL — deliberately not the WebExtension match-
/// pattern spec's scheme/host/path parsing, which is more machinery
/// than a personal userscript file needs. The one host-pattern special
/// case still worth handling: WebExtension match patterns treat
/// `*.example.com` as "example.com itself, or any subdomain of it" —
/// a literal glob `*` doesn't, since it requires the `.` right before
/// `example.com` to actually be there, which a bare `github.com` (no
/// subdomain) doesn't have. Confirmed empirically: `*://*.github.com/*`
/// silently failed to match plain `https://github.com/...`.
fn matches_pattern(pattern: &str, url: &str) -> bool {
    if glob_match(pattern, url) {
        return true;
    }
    // Retry with every `*.` (wildcard-subdomain marker) dropped
    // entirely, so `*://*.github.com/*` also matches the bare apex
    // domain it's almost certainly meant to include.
    if pattern.contains("*.") {
        return glob_match(&pattern.replace("*.", ""), url);
    }
    false
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !text[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if i == parts.len() - 1 {
            return text[pos..].ends_with(part);
        } else {
            match text[pos..].find(part) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
    }
    true
}

/// Every userscript whose `@match` patterns include `url`, in load
/// order — the code to inject into that page, one `execute_java_script`
/// call per match.
pub fn matching<'a, 'b>(
    url: &'b str,
    scripts: &'a [UserScript],
) -> impl Iterator<Item = &'a str> + use<'a, 'b> {
    scripts
        .iter()
        .filter(move |s| {
            s.matches
                .iter()
                .any(|pattern| matches_pattern(pattern, url))
        })
        .map(|s| s.code.as_str())
}
