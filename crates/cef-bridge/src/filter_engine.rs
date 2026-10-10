// EasyList/EasyPrivacy via Brave `adblock`: network cancel, cosmetic CSS,
// optional ##+js. Parsing the lists takes long enough to drop frames, so it
// runs on a background worker and the result is swapped in under ENGINE;
// the IO thread only calls check_request. Needs adblock without
// `single-thread` so Engine is Sync + Send.

use adblock::lists::{FilterFormat, FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
static COSMETIC_ENABLED: AtomicBool = AtomicBool::new(true);
static SCRIPTLETS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Which list files to load from `filters_dir`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterEngineConfig {
    pub easylist: bool,
    pub easyprivacy: bool,
    /// Load `scriptlets.js` into the Engine (needed for ##+js output).
    pub load_scriptlets: bool,
    pub filters_dir: PathBuf,
}

/// Latest-wins request slot shared with the worker thread.
#[derive(Default)]
struct Worker {
    state: Mutex<WorkerState>,
    changed: Condvar,
}

#[derive(Default)]
struct WorkerState {
    /// Config waiting to be built (newer requests overwrite older ones).
    pending: Option<FilterEngineConfig>,
    /// Last config handed to the worker; equal requests are no-ops.
    requested: Option<FilterEngineConfig>,
    /// Builds finished (installed or superseded); lets tests wait.
    completed: u64,
    /// A build is running on the worker right now.
    building: bool,
}

static WORKER: OnceLock<Arc<Worker>> = OnceLock::new();

/// Rebuild the Engine for `config` on a background thread. Returns false
/// when `config` matches the last request (nothing to do) — e.g. a custom
/// host edit, which never touches EasyList.
pub fn rebuild(config: &FilterEngineConfig) -> bool {
    let worker = WORKER.get_or_init(spawn_worker);
    let Ok(mut state) = worker.state.lock() else {
        log::warn!("filter engine worker lock poisoned; rebuild skipped");
        return false;
    };
    if state.requested.as_ref() == Some(config) {
        return false;
    }
    state.requested = Some(config.clone());
    state.pending = Some(config.clone());
    worker.changed.notify_all();
    true
}

/// Block until no rebuild is queued or running, at most `timeout`. Used at
/// startup so restored pages don't load before EasyList is in place.
/// Returns false on timeout.
pub fn wait_idle(timeout: std::time::Duration) -> bool {
    let Some(worker) = WORKER.get() else {
        return true;
    };
    let deadline = std::time::Instant::now() + timeout;
    let Ok(mut state) = worker.state.lock() else {
        return false;
    };
    while state.pending.is_some() || state.building {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return false;
        }
        state = match worker.changed.wait_timeout(state, left) {
            Ok((state, _)) => state,
            Err(_) => return false,
        };
    }
    true
}

fn spawn_worker() -> Arc<Worker> {
    let worker = Arc::new(Worker::default());
    let thread_worker = worker.clone();
    let spawned = std::thread::Builder::new()
        .name("filter-engine".into())
        .spawn(move || worker_loop(&thread_worker));
    if let Err(e) = spawned {
        log::error!("filter engine worker failed to start: {e}");
    }
    worker
}

fn worker_loop(worker: &Worker) {
    loop {
        let config = {
            let Ok(mut state) = worker.state.lock() else {
                return;
            };
            loop {
                if let Some(config) = state.pending.take() {
                    state.building = true;
                    break config;
                }
                state = match worker.changed.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
        };

        let (engine, loaded) = build_engine(&config);

        let Ok(mut state) = worker.state.lock() else {
            return;
        };
        // A newer request arrived mid-build: skip installing this one.
        if state.pending.is_none() {
            install(engine, loaded, &config);
        }
        state.completed += 1;
        state.building = false;
        worker.changed.notify_all();
    }
}

fn install(engine: Option<Engine>, loaded: u32, config: &FilterEngineConfig) {
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

/// Parse the enabled lists into a fresh Engine (slow; never on the UI thread).
fn build_engine(config: &FilterEngineConfig) -> (Option<Engine>, u32) {
    let mut set = FilterSet::new(false);
    let mut loaded = 0u32;

    if config.easylist
        && load_list(&mut set, &config.filters_dir, "easylist.txt", FilterFormat::Standard)
    {
        loaded += 1;
    }
    if config.easyprivacy
        && load_list(
            &mut set,
            &config.filters_dir,
            "easyprivacy.txt",
            FilterFormat::Standard,
        )
    {
        loaded += 1;
    }

    if loaded == 0 {
        return (None, 0);
    }
    let mut engine = Engine::new_with_filter_set(set);
    if config.load_scriptlets {
        attach_scriptlets(&mut engine, &config.filters_dir);
    }
    (Some(engine), loaded)
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

/// Skip ##+js on Twitch/YouTube/Kick (breaks MSE players); cosmetic hide still runs.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "spatial-filter-test-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wait_completed(at_least: u64) {
        let worker = WORKER.get().expect("worker started");
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut state = worker.state.lock().unwrap();
        while state.completed < at_least {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "filter engine build timed out");
            state = worker.changed.wait_timeout(state, left).unwrap().0;
        }
    }

    #[test]
    fn build_engine_loads_enabled_lists_only() {
        let dir = temp_dir("build");
        std::fs::write(dir.join("easylist.txt"), "||ads.example^\n").unwrap();
        let config = FilterEngineConfig {
            easylist: true,
            easyprivacy: true, // file missing → skipped, not fatal
            load_scriptlets: false,
            filters_dir: dir.clone(),
        };
        let (engine, loaded) = build_engine(&config);
        assert_eq!(loaded, 1);
        let engine = engine.expect("engine built");
        let req = Request::new("https://ads.example/x.js", "https://site.test/", "script", "GET")
            .unwrap();
        assert!(engine.check_network_request(&req).should_block());

        let (none, loaded) = build_engine(&FilterEngineConfig {
            easylist: false,
            ..config
        });
        assert!(none.is_none());
        assert_eq!(loaded, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rebuild_runs_in_background_and_dedups() {
        let dir = temp_dir("worker");
        std::fs::write(dir.join("easylist.txt"), "||tracker.example^\n").unwrap();
        let config = FilterEngineConfig {
            easylist: true,
            filters_dir: dir.clone(),
            ..Default::default()
        };
        assert!(rebuild(&config));
        // Same config again (e.g. custom host edit) → no second build.
        assert!(!rebuild(&config));
        wait_completed(1);
        assert!(check_request(
            "https://tracker.example/p.gif",
            "https://site.test/",
            cef::ResourceType::IMAGE
        ));
        let _ = std::fs::remove_dir_all(dir);
    }
}
