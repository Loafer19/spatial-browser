// Load progress, document-start/load-end injection, history records.

use super::*;

/// Load events are drained a frame after CEF raised them; by then the frame
/// may show another origin, which must not get scripts or a save banner
/// (with its password) meant for the queued URL.
pub(super) fn frame_still_on(frame: &cef::Frame, url: &str) -> bool {
    let current = cef::CefString::from(&frame.url()).to_string();
    let same = same_origin(&current, url);
    if !same {
        log::debug!("skip inject for {url}: frame now on {current}");
    }
    same
}

pub(super) fn same_origin(a: &str, b: &str) -> bool {
    // Built-in pages: one tile's refresh-in-place keeps it on `spatial:`.
    match (a.starts_with("spatial:"), b.starts_with("spatial:")) {
        (true, true) => true,
        // Two non-web URLs both normalize to "": not a match.
        (false, false) => {
            let origin = vault::normalize_origin(a);
            !origin.is_empty() && origin == vault::normalize_origin(b)
        }
        _ => false,
    }
}

/// Load state/progress → the top-edge bar (built-in tiles never show one).
pub(super) fn apply_load_progress(session: &mut Session) {
    // Load progress → top-edge bar (skip ephemeral).
    let load_states = PENDING_LOAD_STATE.with_borrow_mut(std::mem::take);
    for (browser_id, is_loading) in load_states {
        if let Some(page) = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == browser_id)
        {
            if page.ephemeral {
                page.load_active.set(false);
                continue;
            }
            page.load_active.set(is_loading);
            if is_loading {
                if page.load_progress.get() >= 1.0 {
                    page.load_progress.set(0.0);
                }
            } else {
                page.load_progress.set(1.0);
                page.load_active.set(false);
            }
        }
    }
    let load_progress = PENDING_LOAD_PROGRESS.with_borrow_mut(std::mem::take);
    for (browser_id, progress) in load_progress {
        if let Some(page) = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == browser_id)
        {
            if page.ephemeral {
                page.load_active.set(false);
                continue;
            }
            let p = (progress as f32).clamp(0.0, 1.0);
            page.load_progress.set(p);
            page.load_active.set(p < 0.999);
        }
    }
}

/// document-start injection: cosmetic CSS, scriptlets, userstyles and
/// document-start userscripts.
pub(super) fn inject_document_start(
    session: &mut Session,
    userscripts: &[UserScript],
    userstyles: &[UserStyle],
) {
    // document-start inject (before page scripts when CEF allows).
    let load_starts = PENDING_LOAD_START.with_borrow_mut(std::mem::take);
    for (browser_id, url) in load_starts {
        if let Some(page) = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == browser_id)
        {
            // Reader mode rewrites the DOM; a real navigation replaces it.
            page.reader_mode.set(false);
            if !page.ephemeral {
                page.load_active.set(true);
                page.load_progress.set(0.0);
            }
            if let Some(frame) = page
                .browser
                .main_frame()
                .filter(|f| frame_still_on(f, &url))
            {
                // EasyList cosmetic hides + optional ##+js scriptlets.
                if !page.ephemeral {
                    if let Some(css) = cef_bridge::cosmetic_hide_css(&url) {
                        let js = cef_bridge::cosmetic_inject_js(&css);
                        frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                    }
                    if let Some(script) = cef_bridge::scriptlet_js(&url) {
                        let js = cef_bridge::scriptlet_inject_js(&script);
                        frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                    }
                }
                // `@match spatial-ui` covers ephemeral chrome pages.
                for js in userstyles::matching_inject_js(&url, page.ephemeral, userstyles) {
                    frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                }
                for code in userscripts::matching_code(
                    &url,
                    page.ephemeral,
                    userscripts,
                    RunAt::DocumentStart,
                ) {
                    frame.execute_java_script(Some(&code.as_str().into()), Some(&"".into()), 0);
                }
            }
        }
    }
}

/// load-end: copy/autofill bridges, document-end/idle userscripts, styles
/// again, and the history record.
pub(super) fn inject_load_end(
    session: &mut Session,
    history: &mut Vec<HistoryEntry>,
    userscripts: &[UserScript],
    userstyles: &[UserStyle],
) {
    // load-end: re-inject copy bridge + document-end/idle scripts/styles;
    // record history (skip ephemeral).
    let visits = PENDING_VISITS.with_borrow_mut(std::mem::take);
    for (browser_id, url) in visits {
        if let Some(page) = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == browser_id)
        {
            if let Some(frame) = page
                .browser
                .main_frame()
                .filter(|f| frame_still_on(f, &url))
            {
                frame.execute_java_script(
                    Some(&clipboard_bridge::COPY_BRIDGE_SCRIPT.into()),
                    Some(&"".into()),
                    0,
                );
                if !page.ephemeral {
                    let bridge = autofill_bridge::script(&session.theme());
                    frame.execute_java_script(Some(&bridge.as_str().into()), Some(&"".into()), 0);
                }
                // Styles again (missed start / full document swap).
                for js in userstyles::matching_inject_js(&url, page.ephemeral, userstyles) {
                    frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                }
                for run_at in [RunAt::DocumentEnd, RunAt::DocumentIdle] {
                    for code in
                        userscripts::matching_code(&url, page.ephemeral, userscripts, run_at)
                    {
                        frame.execute_java_script(Some(&code.as_str().into()), Some(&"".into()), 0);
                    }
                }
            }
            if !page.ephemeral {
                history::record(history, &url, now_unix_secs());
            }
        }
    }
}
