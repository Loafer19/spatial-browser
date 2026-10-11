// Built-in list page actions (bookmarks, omnibox, palette, downloads,
// history, workspaces, settings, userscripts).

use super::query::{percent_decode, query_param};
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

pub(super) fn parse_bookmark_action(url: &str) -> Option<BookmarkAction> {
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

thread_local! {
    /// `omnibox://suggest?q=…` (omnibox browser_id, typed text): answered
    /// by script into that omnibox tile only.
    pub static PENDING_OMNIBOX_SUGGEST: RefCell<Option<(i32, String)>> = const { RefCell::new(None) };
}

pub(super) fn parse_omnibox_suggest(url: &str) -> Option<String> {
    let rest = url.strip_prefix("omnibox://")?;
    let query = rest
        .strip_prefix("suggest?")
        .or_else(|| rest.strip_prefix("suggest/?"))?;
    Some(query_param(query, "q").unwrap_or_default())
}

pub(super) fn parse_omnibox_submit(url: &str) -> Option<OmniboxSubmit> {
    let query = url.strip_prefix("omnibox://go?")?;
    Some(OmniboxSubmit {
        raw: query_param(query, "q")?,
        url: query_param(query, "url")?,
    })
}

/// Action from a `palette://...` navigation (Ctrl+K command palette).
pub enum PaletteAction {
    /// Front and pan to the open tile with this browser id.
    Go(i32),
    /// Run the palette command with this id (on the tile that was focused
    /// when the palette opened).
    Run(String),
    /// Open a bookmark / history URL in a new tile.
    Open(String),
    /// Escape: close the palette.
    Close,
}

thread_local! {
    // (palette browser_id, action)
    pub static PENDING_PALETTE_ACTION: RefCell<Option<(i32, PaletteAction)>> =
        const { RefCell::new(None) };
}

/// Longest command id `palette://run/<id>` accepts.
const MAX_COMMAND_ID_LEN: usize = 40;

/// Public for the compositor's palette tests (ids must survive parsing).
pub fn parse_palette_action(url: &str) -> Option<PaletteAction> {
    let rest = url.strip_prefix("palette://")?;
    if let Some(id) = rest.strip_prefix("go/") {
        return id.trim_end_matches('/').parse().ok().map(PaletteAction::Go);
    }
    if let Some(id) = rest.strip_prefix("run/") {
        let id = id.trim_end_matches('/');
        let valid = !id.is_empty()
            && id.len() <= MAX_COMMAND_ID_LEN
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        return valid.then(|| PaletteAction::Run(id.to_string()));
    }
    // CEF may canonicalize `open?` to `open/?`.
    if let Some(query) = rest
        .strip_prefix("open?")
        .or_else(|| rest.strip_prefix("open/?"))
    {
        return query_param(query, "url")
            .filter(|u| !u.trim().is_empty())
            .map(PaletteAction::Open);
    }
    if rest.trim_end_matches('/') == "close" {
        return Some(PaletteAction::Close);
    }
    None
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

pub(super) fn parse_download_action(url: &str) -> Option<DownloadPageAction> {
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
    /// Every visit of the URL at this index (collapsed "Recent" rows).
    RemoveAll(usize),
    Clear,
    /// `history://search?q=…&offset=N[&collapse=1]`: the next page of
    /// matching rows (one per URL with `collapse`), answered by script into
    /// the asking (built-in) page only.
    Search {
        query: String,
        offset: usize,
        collapse: bool,
    },
}

thread_local! {
    pub static PENDING_HISTORY_ACTION: RefCell<Option<(i32, HistoryPageAction)>> =
        const { RefCell::new(None) };
}

pub(super) fn parse_history_action(url: &str) -> Option<HistoryPageAction> {
    let rest = url.strip_prefix("history://")?;
    if let Some(index) = rest.strip_prefix("open/") {
        return index.parse().ok().map(HistoryPageAction::Open);
    }
    if let Some(index) = rest.strip_prefix("remove/") {
        return index.parse().ok().map(HistoryPageAction::Remove);
    }
    if let Some(index) = rest.strip_prefix("remove-all/") {
        return index.parse().ok().map(HistoryPageAction::RemoveAll);
    }
    if rest == "clear" {
        return Some(HistoryPageAction::Clear);
    }
    // CEF may canonicalize `search?` to `search/?`.
    if let Some(query) = rest
        .strip_prefix("search?")
        .or_else(|| rest.strip_prefix("search/?"))
    {
        return Some(HistoryPageAction::Search {
            query: query_param(query, "q").unwrap_or_default(),
            offset: query_param(query, "offset")
                .and_then(|o| o.parse().ok())
                .unwrap_or(0),
            collapse: query_param(query, "collapse").as_deref() == Some("1"),
        });
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

pub(super) fn parse_workspace_action(url: &str) -> Option<WorkspacePageAction> {
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

pub(super) fn parse_settings_action(url: &str) -> Option<SettingsPageAction> {
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

pub(super) fn parse_userscripts_action(url: &str) -> Option<UserscriptsPageAction> {
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
