// Custom-scheme intercepts (`bookmark://`, `omnibox://`, …): cancel in
// on_before_browse, stash in PENDING_* for the compositor (clipboard://copy
// is handled immediately — no Session needed).

use crate::blocklist::{OsrResourceRequestHandler, ResourceRequestHandlerBuilder};
use crate::clean_urls;
use cef::{self, rc::Rc, *};
use std::cell::RefCell;

/// Action from a `bookmark://...` navigation.
pub enum BookmarkAction {
    Open(usize),
    Delete(usize),
    /// index, new title, new folder (empty string = clear/ungrouped)
    Rename(usize, String, String),
    /// Native file picker → merge Chrome Bookmarks JSON.
    ImportBrowse,
}

thread_local! {
    // (browser_id, action); drained once per frame by the compositor.
    pub static PENDING_BOOKMARK: RefCell<Option<(i32, BookmarkAction)>> = const { RefCell::new(None) };
}

/// Minimal form-urlencoded decode for rename form fields.
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

/// Decoded value of `name` from `a=1&b=2`-style query text.
fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| percent_decode(value))
    })
}

fn parse_bookmark_action(url: &str) -> Option<BookmarkAction> {
    let rest = url.strip_prefix("bookmark://")?;
    let rest = rest.strip_prefix("go/").unwrap_or(rest);
    if rest == "import-browse" || rest == "import" {
        return Some(BookmarkAction::ImportBrowse);
    }
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

/// Omnibox submit: `raw` = typed text; `url` = JS-resolved destination.
pub struct OmniboxSubmit {
    pub raw: String,
    pub url: String,
}

thread_local! {
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
    // (switcher browser_id, target browser_id)
    pub static PENDING_SWITCH: RefCell<Option<(i32, i32)>> = const { RefCell::new(None) };
}

fn parse_switch_target(url: &str) -> Option<i32> {
    url.strip_prefix("switcher://go/")?.parse().ok()
}

/// Action from a `download://...` navigation.
pub enum DownloadPageAction {
    /// Open with the desktop default handler.
    Open(usize),
    /// Drop list entry only (file stays on disk).
    Remove(usize),
}

thread_local! {
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

/// Action from a `history://...` navigation.
pub enum HistoryPageAction {
    Open(usize),
    Remove(usize),
    Clear,
}

thread_local! {
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

/// Action from a `workspace://...` navigation.
pub enum WorkspacePageAction {
    Load(usize),
    Rename(usize, String),
    Delete(usize),
    SaveNew,
}

thread_local! {
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

/// Action from a `settings://...` navigation.
pub enum SettingsPageAction {
    ToggleAdBlock,
    ToggleCleanUrls,
    ToggleFilterNetwork,
    ToggleFilterCosmetic,
    ToggleFilterScriptlets,
    /// `peter_lowe` | `easylist` | `easyprivacy`.
    ToggleFilterList(String),
    SetTab(String),
    /// Fixed search URL template from Settings (not free text).
    SetSearchEngine(String),
    SetTheme(usize),
    /// Index into `reader_mode::READER_THEMES`.
    SetReaderTheme(usize),
    AddBlockedHost(String),
    RemoveBlockedHost(usize),
    /// 60 / 90 / 120 — see `browser::set_target_frame_rate`.
    SetFrameRate(u32),
}

thread_local! {
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
    if rest == "toggle-filter-network" {
        return Some(SettingsPageAction::ToggleFilterNetwork);
    }
    if rest == "toggle-filter-cosmetic" {
        return Some(SettingsPageAction::ToggleFilterCosmetic);
    }
    if rest == "toggle-filter-scriptlets" {
        return Some(SettingsPageAction::ToggleFilterScriptlets);
    }
    if let Some(id) = rest.strip_prefix("toggle-filter-list/") {
        if matches!(id, "peter_lowe" | "easylist" | "easyprivacy") {
            return Some(SettingsPageAction::ToggleFilterList(id.to_string()));
        }
    }
    if let Some(tab) = rest.strip_prefix("tab/") {
        return Some(SettingsPageAction::SetTab(tab.to_string()));
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

/// Action from a `userscripts://...` navigation.
pub enum UserscriptsPageAction {
    Reload,
    OpenDir,
    /// Basename of the `.js` file to flip enabled/disabled.
    Toggle(String),
    /// Open the userstyles folder (styles share the Ctrl+Shift+U page).
    OpenStylesDir,
    /// Basename of a `.css` userstyle to flip enabled/disabled.
    ToggleStyle(String),
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
    if rest == "open-styles-dir" {
        return Some(UserscriptsPageAction::OpenStylesDir);
    }
    if let Some(name) = rest.strip_prefix("toggle-style/") {
        let name = percent_decode(name);
        if !name.is_empty() {
            return Some(UserscriptsPageAction::ToggleStyle(name));
        }
    }
    if let Some(name) = rest.strip_prefix("toggle/") {
        let name = percent_decode(name);
        if !name.is_empty() {
            return Some(UserscriptsPageAction::Toggle(name));
        }
    }
    None
}

/// Vault / autofill actions from `password://...`.
pub enum PasswordAction {
    /// Master password for unlock or create (confirm also set for create).
    Unlock {
        password: String,
        create: bool,
        confirm: Option<String>,
    },
    Query {
        origin: String,
    },
    Fill {
        id: String,
    },
    SaveOffer {
        origin: String,
        username: String,
        password: String,
    },
    Save {
        origin: String,
        username: String,
        password: String,
        id: Option<String>,
    },
    Never {
        origin: String,
    },
    Delete {
        id: String,
    },
    Upsert {
        id: Option<String>,
        origin: String,
        username: String,
        password: String,
        email: Option<String>,
    },
    RemoveNever {
        origin: String,
    },
    Generate {
        length: usize,
        symbols: bool,
    },
    /// Open a native file picker and import Chrome/Bitwarden CSV.
    ImportBrowse,
    /// Switch passwords UI tab (`saved` / `add` / `generator` / `never`).
    Tab {
        id: String,
    },
    /// Open the passwords list after a successful unlock.
    OpenList,
}

thread_local! {
    pub static PENDING_PASSWORD_ACTION: RefCell<Option<(i32, PasswordAction)>> =
        const { RefCell::new(None) };
}

/// Context-menu actions (`context://...`).
pub enum ContextAction {
    /// Element hit-test result from a page (async after right-click).
    Hit {
        link: Option<String>,
        image: Option<String>,
        password_field: bool,
        page_url: String,
    },
    OpenLink(String),
    Copy(String),
    SaveImage(String),
    GenPassword,
    FillPassword,
    ClosePage,
    NewPage,
    SaveWorkspace,
    Reader,
    Dismiss,
}

thread_local! {
    pub static PENDING_CONTEXT_ACTION: RefCell<Option<(i32, ContextAction)>> =
        const { RefCell::new(None) };
}

fn parse_context_action(url: &str) -> Option<ContextAction> {
    let rest = url.strip_prefix("context://")?;
    if let Some(query) = rest.strip_prefix("hit?") {
        return Some(ContextAction::Hit {
            link: query_param(query, "link").filter(|s| !s.is_empty()),
            image: query_param(query, "image").filter(|s| !s.is_empty()),
            password_field: query_param(query, "pwd").as_deref() == Some("1"),
            page_url: query_param(query, "page").unwrap_or_default(),
        });
    }
    if let Some(query) = rest.strip_prefix("open-link?") {
        return Some(ContextAction::OpenLink(query_param(query, "url")?));
    }
    if let Some(query) = rest.strip_prefix("copy?") {
        return Some(ContextAction::Copy(query_param(query, "text")?));
    }
    if let Some(query) = rest.strip_prefix("save-image?") {
        return Some(ContextAction::SaveImage(query_param(query, "url")?));
    }
    Some(match rest {
        "gen-password" => ContextAction::GenPassword,
        "fill-password" => ContextAction::FillPassword,
        "close-page" => ContextAction::ClosePage,
        "new-page" => ContextAction::NewPage,
        "save-workspace" => ContextAction::SaveWorkspace,
        "reader" => ContextAction::Reader,
        "dismiss" => ContextAction::Dismiss,
        _ => return None,
    })
}

/// Accept `action?…` or CEF-canonicalized `action/?…`.
fn password_action_query<'a>(rest: &'a str, action: &str) -> Option<&'a str> {
    let with_q = format!("{action}?");
    let with_slash_q = format!("{action}/?");
    rest.strip_prefix(&with_q)
        .or_else(|| rest.strip_prefix(&with_slash_q))
}

fn parse_password_action(url: &str) -> Option<PasswordAction> {
    let rest = url.strip_prefix("password://")?;
    // Prefer `password://go/<action>?…` so CEF does not turn `query?…` into
    // `query/?…`. Legacy `password://<action>?…` still accepted.
    let rest = rest.strip_prefix("go/").unwrap_or(rest);
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if let Some(query) = password_action_query(rest, "unlock") {
        let password = query_param(query, "password")?;
        let create = query_param(query, "create").as_deref() == Some("1");
        let confirm = query_param(query, "confirm");
        return Some(PasswordAction::Unlock {
            password,
            create,
            confirm,
        });
    }
    if let Some(query) = password_action_query(rest, "query") {
        return Some(PasswordAction::Query {
            origin: query_param(query, "origin")?,
        });
    }
    if let Some(query) = password_action_query(rest, "fill") {
        return Some(PasswordAction::Fill {
            id: query_param(query, "id")?,
        });
    }
    if let Some(query) = password_action_query(rest, "save-offer") {
        return Some(PasswordAction::SaveOffer {
            origin: query_param(query, "origin").unwrap_or_default(),
            username: query_param(query, "username").unwrap_or_default(),
            password: query_param(query, "password").unwrap_or_default(),
        });
    }
    if let Some(query) = password_action_query(rest, "save") {
        let id = query_param(query, "id").filter(|s| !s.is_empty());
        return Some(PasswordAction::Save {
            origin: query_param(query, "origin").unwrap_or_default(),
            username: query_param(query, "username").unwrap_or_default(),
            password: query_param(query, "password").unwrap_or_default(),
            id,
        });
    }
    if let Some(query) = password_action_query(rest, "never") {
        return Some(PasswordAction::Never {
            origin: query_param(query, "origin")?,
        });
    }
    if let Some(query) = password_action_query(rest, "delete") {
        return Some(PasswordAction::Delete {
            id: query_param(query, "id")?,
        });
    }
    if let Some(query) = password_action_query(rest, "upsert") {
        return Some(PasswordAction::Upsert {
            id: query_param(query, "id").filter(|s| !s.is_empty()),
            origin: query_param(query, "origin").unwrap_or_default(),
            username: query_param(query, "username").unwrap_or_default(),
            password: query_param(query, "password").unwrap_or_default(),
            email: query_param(query, "email").filter(|s| !s.is_empty()),
        });
    }
    if let Some(query) = password_action_query(rest, "remove-never") {
        return Some(PasswordAction::RemoveNever {
            origin: query_param(query, "origin")?,
        });
    }
    if let Some(query) = password_action_query(rest, "generate") {
        let length = query_param(query, "length")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        let symbols = query_param(query, "symbols").as_deref() != Some("0");
        return Some(PasswordAction::Generate { length, symbols });
    }
    if rest == "import-browse" || rest == "import" {
        return Some(PasswordAction::ImportBrowse);
    }
    if let Some(id) = rest.strip_prefix("tab/") {
        return Some(PasswordAction::Tab {
            id: id.to_string(),
        });
    }
    if rest == "open-list" {
        return Some(PasswordAction::OpenList);
    }
    None
}

/// Parse `clipboard://copy?text=...` (OSR has no OS clipboard surface).
fn parse_clipboard_copy(url: &str) -> Option<String> {
    let query = url.strip_prefix("clipboard://copy?")?;
    query_param(query, "text")
}

/// Spawn `wl-copy` (daemonizes; do not wait).
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
            // Skip fake action schemes; they never carry tracking params.
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
            if let Some(action) = parse_password_action(&url) {
                PENDING_PASSWORD_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(action) = parse_context_action(&url) {
                PENDING_CONTEXT_ACTION.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _;
            }
            if let Some(text) = parse_clipboard_copy(&url) {
                write_to_clipboard(&text);
                return true as _;
            }
            false as _
        }

        // Blocklist applies to every request, not only top-level navigations.
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
