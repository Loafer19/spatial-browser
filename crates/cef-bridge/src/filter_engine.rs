// EasyList / EasyPrivacy via Brave's `adblock` crate: network cancel,
// cosmetic hide CSS, and optional ##+js scriptlets (classic uBO
// scriptlets.js assembled with `resource-assembler`).
//
// Engine is rebuilt on the UI thread when Settings change; CEF's IO
// thread only calls `check_request`. Requires adblock without the
// `single-thread` feature so Engine is Sync.

use adblock::lists::{FilterFormat, FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
static COSMETIC_ENABLED: AtomicBool = AtomicBool::new(true);
static SCRIPTLETS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Which list files to load from `filters_dir`.
#[derive(Clone, Debug, Default)]
pub struct FilterEngineConfig {
    pub easylist: bool,
    pub easyprivacy: bool,
    /// Load `scriptlets.js` into the Engine (needed for ##+js output).
    pub load_scriptlets: bool,
    pub filters_dir: PathBuf,
}

/// Rebuild the shared Engine from disk. Safe to call from the UI thread;
/// IO-thread checks see the new engine after the lock is released.
pub fn rebuild(config: &FilterEngineConfig) {
    let mut set = FilterSet::new(false);
    let mut loaded = 0u32;

    if config.easylist {
        if load_list(&mut set, &config.filters_dir, "easylist.txt", FilterFormat::Standard) {
            loaded += 1;
        }
    }
    if config.easyprivacy {
        if load_list(
            &mut set,
            &config.filters_dir,
            "easyprivacy.txt",
            FilterFormat::Standard,
        ) {
            loaded += 1;
        }
    }

    let engine = if loaded == 0 {
        None
    } else {
        let mut engine = Engine::new_with_filter_set(set);
        if config.load_scriptlets {
            attach_scriptlets(&mut engine, &config.filters_dir);
        }
        Some(engine)
    };

    match ENGINE.lock() {
        Ok(mut slot) => {
            *slot = engine;
            if loaded > 0 {
                log::info!(
                    "filter engine rebuilt ({loaded} list(s), scriptlets={}) from {}",
                    config.load_scriptlets,
                    config.filters_dir.display()
                );
            } else {
                log::info!("filter engine cleared (no EasyList lists enabled or files missing)");
            }
        }
        Err(e) => log::warn!("filter engine rebuild: lock poisoned: {e}"),
    }
}

fn load_list(set: &mut FilterSet, dir: &Path, name: &str, format: FilterFormat) -> bool {
    let path = dir.join(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            set.add_filter_list(
                text,
                ParseOptions {
                    format,
                    ..ParseOptions::default()
                },
            );
            true
        }
        Err(e) => {
            log::warn!("filter list {}: {e}", path.display());
            false
        }
    }
}

fn attach_scriptlets(engine: &mut Engine, dir: &Path) {
    let path = dir.join("scriptlets.js");
    if !path.is_file() {
        log::warn!("scriptlets.js missing at {}", path.display());
        return;
    }
    #[allow(deprecated)]
    let resources =
        adblock::resources::resource_assembler::assemble_scriptlet_resources(&path);
    log::info!("loaded {} scriptlet resources", resources.len());
    engine.use_resources(resources);
}

fn map_resource_type(rt: cef::ResourceType) -> &'static str {
    use cef::ResourceType as RT;
    if rt == RT::MAIN_FRAME {
        "document"
    } else if rt == RT::SUB_FRAME {
        "sub_frame"
    } else if rt == RT::STYLESHEET {
        "stylesheet"
    } else if rt == RT::SCRIPT {
        "script"
    } else if rt == RT::IMAGE {
        "image"
    } else if rt == RT::FONT_RESOURCE {
        "font"
    } else if rt == RT::XHR {
        "xmlhttprequest"
    } else if rt == RT::MEDIA {
        "media"
    } else if rt == RT::PING {
        "ping"
    } else if rt == RT::CSP_REPORT {
        "csp_report"
    } else if rt == RT::FAVICON {
        "image"
    } else {
        "other"
    }
}

pub fn set_cosmetic_enabled(enabled: bool) {
    COSMETIC_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_scriptlets_enabled(enabled: bool) {
    SCRIPTLETS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Returns true if an enabled EasyList/EasyPrivacy rule says to block.
pub fn check_request(url: &str, source_url: &str, resource_type: cef::ResourceType) -> bool {
    let Ok(guard) = ENGINE.lock() else {
        return false;
    };
    let Some(engine) = guard.as_ref() else {
        return false;
    };
    let source = if source_url.is_empty() { url } else { source_url };
    let Ok(req) = Request::new(url, source, map_resource_type(resource_type), "GET") else {
        return false;
    };
    engine.check_network_request(&req).should_block()
}

/// Build a stylesheet of `{ display:none !important }` rules for `url`.
pub fn cosmetic_hide_css(url: &str) -> Option<String> {
    if !COSMETIC_ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    let Ok(guard) = ENGINE.lock() else {
        return None;
    };
    let engine = guard.as_ref()?;
    let resources = engine.url_cosmetic_resources(url);
    if resources.hide_selectors.is_empty() {
        return None;
    }
    let mut css = String::new();
    let mut first = true;
    for sel in &resources.hide_selectors {
        if sel.is_empty() || sel.contains('}') || sel.contains('<') {
            continue;
        }
        if !first {
            css.push(',');
        }
        first = false;
        css.push_str(sel);
    }
    if first {
        return None;
    }
    css.push_str("{display:none!important;}");
    Some(css)
}

/// Hosts where ##+js injection is known to break the main player
/// (Twitch Error #4000, similar MSE players). Cosmetic hide still runs.
fn scriptlet_denied(url: &str) -> bool {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches("www.");
    matches!(
        host,
        "twitch.tv"
            | "player.twitch.tv"
            | "m.twitch.tv"
            | "clips.twitch.tv"
            | "kick.com"
            | "youtube.com"
            | "youtu.be"
            | "www.youtube.com"
            | "m.youtube.com"
    ) || host.ends_with(".twitch.tv")
}

/// Ready-to-run JS from `##+js(...)` rules for `url`, if scriptlets are on.
pub fn scriptlet_js(url: &str) -> Option<String> {
    if !SCRIPTLETS_ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    if scriptlet_denied(url) {
        return None;
    }
    let Ok(guard) = ENGINE.lock() else {
        return None;
    };
    let engine = guard.as_ref()?;
    let resources = engine.url_cosmetic_resources(url);
    let script = resources.injected_script;
    if script.trim().is_empty() {
        return None;
    }
    Some(script)
}

/// Wrap raw scriptlet source so a single failure cannot break the page.
pub fn scriptlet_inject_js(script: &str) -> String {
    format!("(function(){{try{{\n{script}\n}}catch(e){{}}}})();")
}

/// JS snippet that injects `css` as a `<style data-spatial-cosmetic>` node.
pub fn cosmetic_inject_js(css: &str) -> String {
    let payload = js_string_literal(css);
    format!(
        "(function(){{try{{var c={payload};var s=document.createElement('style');\
         s.setAttribute('data-spatial-cosmetic','1');s.textContent=c;\
         (document.documentElement||document.head||document).appendChild(s);\
         }}catch(e){{}}}})();"
    )
}

fn js_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
