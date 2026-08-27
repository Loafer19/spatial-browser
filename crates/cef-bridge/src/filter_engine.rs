// EasyList / EasyPrivacy network matching via Brave's `adblock` crate.
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

/// Which list files to load from `filters_dir`.
#[derive(Clone, Debug, Default)]
pub struct FilterEngineConfig {
    pub easylist: bool,
    pub easyprivacy: bool,
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
        Some(Engine::new_with_filter_set(set))
    };

    match ENGINE.lock() {
        Ok(mut slot) => {
            *slot = engine;
            if loaded > 0 {
                log::info!(
                    "filter engine rebuilt ({loaded} list(s) from {})",
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

fn map_resource_type(rt: cef::ResourceType) -> &'static str {
    use cef::ResourceType as RT;
    // CEF ResourceType is a newtype; compare against known constants.
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

/// Whether cosmetic CSS inject is allowed (Settings → Advanced).
pub fn set_cosmetic_enabled(enabled: bool) {
    COSMETIC_ENABLED.store(enabled, Ordering::Relaxed);
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
/// Returns `None` when cosmetic is off, the engine is empty, or there
/// are no hide selectors. Generic class/id follow-up
/// (`hidden_class_id_selectors`) is not wired yet — URL-specific hides
/// only.
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
        if sel.is_empty() {
            continue;
        }
        // Skip selectors that would break out of our style rule.
        if sel.contains('}') || sel.contains('<') {
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
