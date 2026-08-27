// EasyList / EasyPrivacy network matching via Brave's `adblock` crate.
// Engine is rebuilt on the UI thread when Settings change; CEF's IO
// thread only calls `check_request`. Requires adblock without the
// `single-thread` feature so Engine is Sync.

use adblock::lists::{FilterFormat, FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

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
