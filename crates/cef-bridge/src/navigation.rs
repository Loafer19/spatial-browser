// Custom-scheme interception: every generated list page (bookmarks,
// omnibox, the page switcher, downloads, history, workspaces,
// settings) signals an action back to the compositor by navigating to
// a fake `whatever://...` URL rather than a real one —
// `OsrRequestHandler::on_before_browse` cancels that navigation and
// parses it into one of the types below, left in a thread_local for
// the compositor's redraw handler to act on (same shape for six of
// them: `RefCell<Option<(browser_id, Action)>>`, read+cleared once per
// frame — `clipboard://copy` is the exception, acted on immediately
// right here since it needs no canvas/Session state at all). Grouped
// into one file, unlike the other handlers here (one file each),
// because these aren't separate CEF interfaces — they're all
// `RequestHandler::on_before_browse`, dispatching on a URL prefix —
// and the whole point of a shared file is that dispatch living next to
// every action it dispatches to.

use crate::blocklist::{OsrResourceRequestHandler, ResourceRequestHandlerBuilder};
use crate::clean_urls;
use cef::{self, rc::Rc, *};
use std::cell::RefCell;

/// What the bookmarks-list page (compositor::hotkeys) asked for, parsed
/// from a `bookmark://...` link/form it navigated to.
pub enum BookmarkAction {
    Open(usize),
    Delete(usize),
    /// index, new title, new folder (empty string = clear/ungrouped)
    Rename(usize, String, String),
}

thread_local! {
    // Set by `OsrRequestHandler::on_before_browse` when a page navigates
    // to a `bookmark://...` link or form (only the generated
    // bookmarks-list page ever produces one — see compositor::hotkeys —
    // so a global slot is safe: no other page's real navigation can
    // collide with it). The browser identifier (CEF's own per-instance
    // id) tags *which* page asked, so the compositor can reload that
    // exact bookmarks-list page in place after a delete/rename rather
    // than guessing. Read once per frame by the compositor's redraw
    // handler.
    pub static PENDING_BOOKMARK: RefCell<Option<(i32, BookmarkAction)>> = const { RefCell::new(None) };
}

/// Minimal `application/x-www-form-urlencoded` decode (`+` -> space,
/// `%XX` -> byte) — just enough for the rename form's `title`/`folder`
/// values, without pulling in a URL crate for it.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Pulls a query parameter's decoded value out of `a=1&b=2`-style text.
fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| percent_decode(value))
    })
}

fn parse_bookmark_action(url: &str) -> Option<BookmarkAction> {
    let rest = url.strip_prefix("bookmark://")?;
    if let Some(index) = rest.strip_prefix("open/") {
        return index.parse().ok().map(BookmarkAction::Open);
    }
    if let Some(index) = rest.strip_prefix("delete/") {
        return index.parse().ok().map(BookmarkAction::Delete);
    }
    if let Some(after) = rest.strip_prefix("rename/") {
        let (index, query) = after.split_once('?').unwrap_or((after, ""));
        let index = index.parse().ok()?;
        let title = query_param(query, "title").unwrap_or_default();
        let folder = query_param(query, "folder").unwrap_or_default();
        return Some(BookmarkAction::Rename(index, title, folder));
    }
    None
}

/// What the omnibox page (compositor::hotkeys::omnibox_page_url) asked
/// for, parsed from an `omnibox://go?q=...&url=...` navigation it made.
/// `raw` is exactly what the user typed (for history); `url` is what the
/// page's own JS already resolved it to (URL detection / @prefix search
/// engines / default search) — the compositor doesn't need to know that
/// resolution logic, just where to navigate.
pub struct OmniboxSubmit {
    pub raw: String,
    pub url: String,
}

thread_local! {
    // Same shape/reasoning as PENDING_BOOKMARK, for the omnibox page.
    pub static PENDING_OMNIBOX: RefCell<Option<(i32, OmniboxSubmit)>> = const { RefCell::new(None) };
}

fn parse_omnibox_submit(url: &str) -> Option<OmniboxSubmit> {
    let query = url.strip_prefix("omnibox://go?")?;
    Some(OmniboxSubmit {
        raw: query_param(query, "q")?,
        url: query_param(query, "url")?,
    })
}

thread_local! {
    // Same shape/reasoning as PENDING_BOOKMARK, for the Ctrl+K page
    // switcher. Holds (switcher page's own browser id, target page's
    // browser id) — the compositor brings the target to front and pans
    // to it, then closes the switcher page.
    pub static PENDING_SWITCH: RefCell<Option<(i32, i32)>> = const { RefCell::new(None) };
}

fn parse_switch_target(url: &str) -> Option<i32> {
    url.strip_prefix("switcher://go/")?.parse().ok()
}

/// What the downloads-list page (compositor::hotkeys) asked for, parsed
/// from a `download://...` link.
pub enum DownloadPageAction {
    /// Opens the downloaded file with the desktop's default handler.
    Open(usize),
    /// Removes the entry from the list (the downloaded file itself is
    /// left on disk — this only forgets about it).
    Remove(usize),
}

thread_local! {
    // Same shape/reasoning as PENDING_BOOKMARK, for the downloads-list page.
    pub static PENDING_DOWNLOAD_ACTION: RefCell<Option<(i32, DownloadPageAction)>> =
        const { RefCell::new(None) };
}

fn parse_download_action(url: &str) -> Option<DownloadPageAction> {
    let rest = url.strip_prefix("download://")?;
    if let Some(index) = rest.strip_prefix("open/") {
        return index.parse().ok().map(DownloadPageAction::Open);
    }
    if let Some(index) = rest.strip_prefix("remove/") {
        return index.parse().ok().map(DownloadPageAction::Remove);
    }
    None
}

/// What the history-list page (compositor::hotkeys) asked for, parsed
/// from a `history://...` link.
pub enum HistoryPageAction {
    /// Opens the visited URL as a new page on the canvas.
    Open(usize),
    /// Removes one entry from the list.
    Remove(usize),
    /// Removes every entry.
    Clear,
}

thread_local! {
    // Same shape/reasoning as PENDING_BOOKMARK, for the history-list page.
    pub static PENDING_HISTORY_ACTION: RefCell<Option<(i32, HistoryPageAction)>> =
        const { RefCell::new(None) };
}

fn parse_history_action(url: &str) -> Option<HistoryPageAction> {
    let rest = url.strip_prefix("history://")?;
    if let Some(index) = rest.strip_prefix("open/") {
        return index.parse().ok().map(HistoryPageAction::Open);
    }
    if let Some(index) = rest.strip_prefix("remove/") {
        return index.parse().ok().map(HistoryPageAction::Remove);
    }
    if rest == "clear" {
        return Some(HistoryPageAction::Clear);
    }
    None
}

/// What the workspace-list page (compositor::hotkeys) asked for,
/// parsed from a `workspace://...` link/form.
pub enum WorkspacePageAction {
    /// Replaces every currently open page with the ones saved in this
    /// workspace.
    Load(usize),
    /// New display name.
    Rename(usize, String),
    Delete(usize),
    /// Snapshots the current canvas (pages/viewport/theme) as a new
    /// workspace entry.
    SaveNew,
}

thread_local! {
    // Same shape/reasoning as PENDING_BOOKMARK, for the workspace-list page.
    pub static PENDING_WORKSPACE_ACTION: RefCell<Option<(i32, WorkspacePageAction)>> =
        const { RefCell::new(None) };
}

fn parse_workspace_action(url: &str) -> Option<WorkspacePageAction> {
    let rest = url.strip_prefix("workspace://")?;
    if let Some(index) = rest.strip_prefix("load/") {
        return index.parse().ok().map(WorkspacePageAction::Load);
    }
    if let Some(after) = rest.strip_prefix("rename/") {
        let (index, query) = after.split_once('?').unwrap_or((after, ""));
        let index = index.parse().ok()?;
        let name = query_param(query, "name").unwrap_or_default();
        return Some(WorkspacePageAction::Rename(index, name));
    }
    if let Some(index) = rest.strip_prefix("delete/") {
        return index.parse().ok().map(WorkspacePageAction::Delete);
    }
    if rest == "save" {
        return Some(WorkspacePageAction::SaveNew);
    }
    None
}

/// What the settings page (compositor::hotkeys) asked for, parsed from
/// a `settings://...` link/form.
pub enum SettingsPageAction {
    ToggleAdBlock,
    ToggleCleanUrls,
    /// A full search URL template (e.g. `https://www.bing.com/search?q=`)
    /// — one of the settings page's own fixed choices, not free text.
    SetSearchEngine(String),
    SetTheme(usize),
    /// Index into compositor::reader_mode::READER_THEMES — which of
    /// reader mode's Light/Sepia/Dark colors Ctrl+Shift+R uses.
    SetReaderTheme(usize),
    AddBlockedHost(String),
    RemoveBlockedHost(usize),
    /// One of the settings page's own fixed choices (60/90/120), not
    /// free text — see compositor::browser::set_target_frame_rate.
    SetFrameRate(u32),
}

thread_local! {
    // Same shape/reasoning as PENDING_BOOKMARK, for the settings page.
    pub static PENDING_SETTINGS_ACTION: RefCell<Option<(i32, SettingsPageAction)>> =
        const { RefCell::new(None) };
}

fn parse_settings_action(url: &str) -> Option<SettingsPageAction> {
    let rest = url.strip_prefix("settings://")?;
    if rest == "toggle-adblock" {
        return Some(SettingsPageAction::ToggleAdBlock);
    }
    if rest == "toggle-clean-urls" {
        return Some(SettingsPageAction::ToggleCleanUrls);
    }
    if let Some(query) = rest.strip_prefix("search-engine?") {
        let engine = query_param(query, "engine")?;
        return Some(SettingsPageAction::SetSearchEngine(engine));
    }
    if let Some(index) = rest.strip_prefix("theme/") {
        return index.parse().ok().map(SettingsPageAction::SetTheme);
    }
    if let Some(index) = rest.strip_prefix("reader-theme/") {
        return index.parse().ok().map(SettingsPageAction::SetReaderTheme);
    }
    if let Some(query) = rest.strip_prefix("add-host?") {
        let host = query_param(query, "host")?;
        return Some(SettingsPageAction::AddBlockedHost(host));
    }
    if let Some(index) = rest.strip_prefix("remove-host/") {
        return index
            .parse()
            .ok()
            .map(SettingsPageAction::RemoveBlockedHost);
    }
    if let Some(fps) = rest.strip_prefix("frame-rate/") {
        return fps.parse().ok().map(SettingsPageAction::SetFrameRate);
    }
    None
}

/// What the userscripts list page asked for, parsed from a
/// `userscripts://...` link.
pub enum UserscriptsPageAction {
    Reload,
    OpenDir,
    /// Basename of the `.js` file to flip enabled/disabled.
    Toggle(String),
}

thread_local! {
    pub static PENDING_USERSCRIPT_ACTION: RefCell<Option<(i32, UserscriptsPageAction)>> =
        const { RefCell::new(None) };
}

fn parse_userscripts_action(url: &str) -> Option<UserscriptsPageAction> {
    let rest = url.strip_prefix("userscripts://")?;
    if rest == "reload" {
        return Some(UserscriptsPageAction::Reload);
    }
    if rest == "open-dir" {
        return Some(UserscriptsPageAction::OpenDir);
    }
    if let Some(name) = rest.strip_prefix("toggle/") {
        let name = percent_decode(name);
        if !name.is_empty() {
            return Some(UserscriptsPageAction::Toggle(name));
        }
    }
    None
}

/// Parses a `clipboard://copy?text=...` navigation into the copied
/// text — sent by the copy-bridge script every page has injected into
/// it (compositor::clipboard_bridge), since CEF's windowless/OSR
/// Chromium has no real platform surface to write the actual OS
/// clipboard through itself (confirmed empirically: copied text inside
/// a page never reached `wl-paste`). Unlike every other action in this
/// file, this one is acted on immediately, right here — writing to the
/// clipboard needs no canvas/Session state at all, so there's no
/// PENDING_* thread_local or compositor round-trip for it.
fn parse_clipboard_copy(url: &str) -> Option<String> {
    let query = url.strip_prefix("clipboard://copy?")?;
    query_param(query, "text")
}

/// Hands `text` to `wl-copy`, which daemonizes itself to keep serving
/// paste requests — spawned and left running, not waited on.
fn write_to_clipboard(text: &str) {
    if let Err(e) = std::process::Command::new("wl-copy").arg(text).spawn() {
        log::warn!("wl-copy failed (system clipboard copy unavailable): {e}");
    }
}

#[derive(Clone)]
pub struct OsrRequestHandler {}

wrap_request_handler! {
    pub struct RequestHandlerBuilder {
        handler: OsrRequestHandler,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let (Some(browser), Some(request)) = (browser, request) else {
                return false as _;
            };
            let url = cef::CefString::from(&request.url()).to_string();
            let id = browser.identifier();
            // Only ever rewrites real navigations, never this file's own
            // fake `whatever://...` action-signaling URLs — those never
            // carry tracking params anyway, but the scheme check keeps
            // this from even looking.
            if url.starts_with("http://") || url.starts_with("https://") {
                if let Some(cleaned) = clean_urls::clean(&url) {
                    if let Some(frame) = frame {
                        frame.load_url(Some(&cleaned.as_str().into()));
                    }
                    return true as _; // cancel — the load_url above replaces it
                }
            }
            if let Some(action) = parse_bookmark_action(&url) {
                PENDING_BOOKMARK.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _; // cancel navigation — the compositor acts on it instead
            }
            if let Some(submit) = parse_omnibox_submit(&url) {
                PENDING_OMNIBOX.with_borrow_mut(|pending| *pending = Some((id, submit)));
                return true as _;
            }
            if let Some(target_id) = parse_switch_target(&url) {
                PENDING_SWITCH.with_borrow_mut(|pending| *pending = Some((id, target_id)));
                return true as _;
            }
            if let Some(action) = parse_download_action(&url) {
                PENDING_DOWNLOAD_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_history_action(&url) {
                PENDING_HISTORY_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_workspace_action(&url) {
                PENDING_WORKSPACE_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
                    if let Some(action) = parse_settings_action(&url) {
                PENDING_SETTINGS_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_userscripts_action(&url) {
                PENDING_USERSCRIPT_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(text) = parse_clipboard_copy(&url) {
                write_to_clipboard(&text);
                return true as _;
            }
            false as _
        }

        // Ad/tracker request blocking (see blocklist.rs) — every
        // request, not just top-level navigations, needs this same
        // handler, so it's returned unconditionally rather than only
        // for some subset.
        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<cef::ResourceRequestHandler> {
            Some(ResourceRequestHandlerBuilder::build(
                OsrResourceRequestHandler {},
            ))
        }
    }
}

impl RequestHandlerBuilder {
    pub fn build(handler: OsrRequestHandler) -> cef::RequestHandler {
        Self::new(handler)
    }
}
