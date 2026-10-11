// Everything a shortcut or the Ctrl+K palette can do. Hotkeys map a key to a
// `Command` (hotkeys::command_for), the palette names one by id
// (`palette://run/<id>`, see PALETTE), and both go through `run` — one
// implementation per action, whichever way it was asked for.

use crate::hotkeys;
use crate::output::GpuState;
use crate::pages::ui::Icon;
use crate::pending_actions::{self, Stores};
use crate::persistence::workspaces;
use crate::session::Session;
use crate::userscripts;
use crate::userstyles;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Command {
    // Pages
    NewPage,
    ReopenClosed,
    ClosePage,
    Reload,
    FindInPage,
    OpenInSystemBrowser,
    Paste,
    CopyUrl,
    ReaderMode,
    Back,
    Forward,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    // Lists
    BookmarkPage,
    Bookmarks,
    Downloads,
    History,
    Palette,
    Workspaces,
    NewWorkspace,
    /// Workspace slot 1–9.
    SwitchWorkspace(u8),
    /// Move the focused page to workspace slot 1–9 and follow it.
    MoveToWorkspace(u8),
    Scripts,
    Passwords,
    LockVault,
    Settings,
    // Canvas
    AutoLayout,
    NextPage,
    PreviousPage,
    ZoomToCanvas,
    ResetCanvas,
    // Other
    CycleTheme,
    Help,
}

impl Command {
    /// Acts on the focused (topmost) page rather than opening something or
    /// changing the canvas. From the palette these need the page that was
    /// focused when it opened; if that page is gone they do nothing.
    pub(crate) fn targets_page(self) -> bool {
        matches!(
            self,
            Command::ClosePage
                | Command::Reload
                | Command::FindInPage
                | Command::OpenInSystemBrowser
                | Command::Paste
                | Command::CopyUrl
                | Command::ReaderMode
                | Command::Back
                | Command::Forward
                | Command::ZoomIn
                | Command::ZoomOut
                | Command::ZoomReset
                | Command::BookmarkPage
                | Command::MoveToWorkspace(_)
                | Command::ZoomToCanvas
        )
    }

    /// The palette entry for this command, if it is listed there.
    #[cfg(test)]
    pub(crate) fn palette_entry(self) -> Option<&'static PaletteCommand> {
        PALETTE.iter().find(|c| c.command == self)
    }

    /// `palette://run/<id>` → command (only commands the palette lists).
    pub(crate) fn from_id(id: &str) -> Option<Command> {
        PALETTE.iter().find(|c| c.id == id).map(|c| c.command)
    }
}

/// One row of the palette's Commands section.
pub(crate) struct PaletteCommand {
    /// `palette://run/<id>`: `[a-z0-9-]`, unique.
    pub id: &'static str,
    pub command: Command,
    pub title: &'static str,
    /// Extra words the fuzzy match looks at (not shown).
    pub keywords: &'static str,
    /// As shown in F1 help; `+`-separated keys.
    pub shortcut: Option<&'static str>,
    pub icon: Icon,
    /// Listed when the query is empty.
    pub featured: bool,
}

const fn entry(
    id: &'static str,
    command: Command,
    title: &'static str,
    keywords: &'static str,
    shortcut: &'static str,
    icon: Icon,
    featured: bool,
) -> PaletteCommand {
    PaletteCommand {
        id,
        command,
        title,
        keywords,
        shortcut: if shortcut.is_empty() {
            None
        } else {
            Some(shortcut)
        },
        icon,
        featured,
    }
}

/// Commands the palette lists, in empty-query order (featured ones first
/// matter most). Repeat-style actions (page zoom, next page, paste) stay
/// keyboard-only: from a list they are slower than the key.
#[rustfmt::skip]
pub(crate) const PALETTE: &[PaletteCommand] = &[
    entry("new-page", Command::NewPage, "New page", "tab open url address search omnibox", "Ctrl+T", Icon::Add, true),
    entry("reopen-closed", Command::ReopenClosed, "Reopen closed page", "undo restore tab", "Ctrl+Shift+T", Icon::Refresh, true),
    entry("find-in-page", Command::FindInPage, "Find in page", "search text", "Ctrl+F", Icon::Search, true),
    entry("bookmarks", Command::Bookmarks, "Bookmarks", "favorites saved list", "Ctrl+B", Icon::Star, true),
    entry("history", Command::History, "History", "visited recent", "Ctrl+H", Icon::History, true),
    entry("downloads", Command::Downloads, "Downloads", "files", "Ctrl+J", Icon::Download, true),
    entry("settings", Command::Settings, "Settings", "preferences options config adblock search engine", "Ctrl+,", Icon::Gear, true),
    entry("auto-layout", Command::AutoLayout, "Auto-layout grid", "arrange tile tidy pages", "Ctrl+G", Icon::Grid, true),
    entry("passwords", Command::Passwords, "Passwords", "vault logins credentials unlock", "Ctrl+Shift+P", Icon::Key, true),
    entry("workspaces", Command::Workspaces, "Workspaces", "spaces sessions slots", "Ctrl+Shift+W", Icon::Layers, false),
    entry("new-workspace", Command::NewWorkspace, "New workspace", "space slot", "Ctrl+N", Icon::Layers, false),
    entry("bookmark-page", Command::BookmarkPage, "Bookmark page", "star save favorite", "Ctrl+D", Icon::Star, false),
    entry("copy-url", Command::CopyUrl, "Copy page URL", "link clipboard address", "Ctrl+Shift+C", Icon::Copy, false),
    entry("open-external", Command::OpenInSystemBrowser, "Open in system browser", "external default firefox chrome", "Ctrl+Shift+O", Icon::Open, false),
    entry("reader-mode", Command::ReaderMode, "Toggle reader mode", "article read clean", "Ctrl+Shift+R", Icon::Eye, false),
    entry("reload", Command::Reload, "Reload page", "refresh", "Ctrl+R", Icon::Refresh, false),
    entry("close-page", Command::ClosePage, "Close page", "tab", "Ctrl+W", Icon::Close, false),
    entry("back", Command::Back, "Back", "previous history", "Alt+Left", Icon::History, false),
    entry("forward", Command::Forward, "Forward", "next history", "Alt+Right", Icon::History, false),
    entry("zoom-to-canvas", Command::ZoomToCanvas, "Zoom page to canvas", "fill maximize fullscreen focus", "Ctrl+Space", Icon::Zap, false),
    entry("reset-canvas", Command::ResetCanvas, "Reset canvas view", "center origin home pan zoom", "Ctrl+Shift+0", Icon::Globe, false),
    entry("cycle-theme", Command::CycleTheme, "Cycle theme", "color dark light appearance", "Ctrl+Shift+Space", Icon::Eye, false),
    entry("scripts", Command::Scripts, "Scripts & styles", "userscripts userstyles css js", "Ctrl+Shift+U", Icon::Code, false),
    entry("lock-vault", Command::LockVault, "Lock password vault", "passwords secure", "Ctrl+Shift+L", Icon::Lock, false),
    entry("help", Command::Help, "Keyboard shortcuts", "help keys hotkeys", "F1", Icon::Info, false),
];

/// Do `command` on the focused (topmost) page / the canvas.
pub(crate) fn run(command: Command, session: &mut Session, gpu: &GpuState, s: &mut Stores<'_>) {
    match command {
        Command::NewPage => hotkeys::open_new(session, gpu, s.typed_history, s.settings),
        Command::ReopenClosed => hotkeys::reopen_closed(session, gpu),
        Command::ClosePage => session.close_topmost(),
        Command::Reload => hotkeys::reload_focused(session),
        Command::FindInPage => hotkeys::open_find(session, gpu),
        Command::OpenInSystemBrowser => hotkeys::open_focused_externally(session),
        Command::Paste => hotkeys::paste_into_focused(session),
        Command::CopyUrl => hotkeys::copy_focused_url(session),
        Command::ReaderMode => hotkeys::toggle_reader_mode(session, s.settings),
        Command::Back => hotkeys::go_back(session),
        Command::Forward => hotkeys::go_forward(session),
        Command::ZoomIn => hotkeys::page_zoom(session, cef::ZoomCommand::IN),
        Command::ZoomOut => hotkeys::page_zoom(session, cef::ZoomCommand::OUT),
        Command::ZoomReset => hotkeys::page_zoom(session, cef::ZoomCommand::RESET),
        Command::BookmarkPage => hotkeys::bookmark_focused(session, s.bookmarks),
        Command::Bookmarks => hotkeys::open_bookmarks(session, gpu, s.bookmarks),
        Command::Downloads => hotkeys::open_downloads(session, gpu, s.downloads),
        Command::History => hotkeys::open_history(session, gpu, s.history),
        Command::Palette => {
            hotkeys::toggle_palette(session, gpu, s.bookmarks, s.history, s.settings)
        }
        Command::Workspaces => hotkeys::open_workspaces(session, gpu, s.workspaces),
        Command::NewWorkspace => {
            workspaces::add_and_switch(s.workspaces, s.workspace_runtime, session, gpu);
        }
        Command::SwitchWorkspace(slot) => {
            workspaces::switch_to(s.workspaces, s.workspace_runtime, session, gpu, slot.into());
        }
        Command::MoveToWorkspace(slot) => {
            workspaces::move_focused_to_slot(
                s.workspaces,
                s.workspace_runtime,
                session,
                gpu,
                slot.into(),
            );
        }
        // Opening reloads from disk so new files appear without restart.
        Command::Scripts => {
            userscripts::reload(s.userscripts);
            userstyles::reload(s.userstyles);
            hotkeys::open_userscripts(session, gpu, s.userscripts, s.userstyles);
        }
        Command::Passwords => hotkeys::open_passwords(session, gpu, s.vault.as_ref()),
        // Applied next frame, where the save-offer/generator secrets are in reach.
        Command::LockVault => pending_actions::request_vault_lock(),
        Command::Settings => hotkeys::open_settings(session, gpu, s.settings),
        Command::AutoLayout => hotkeys::auto_layout(session, gpu),
        Command::NextPage => session.rotate_focus(false),
        Command::PreviousPage => session.rotate_focus(true),
        Command::ZoomToCanvas => hotkeys::zoom_to_canvas(session, gpu),
        Command::ResetCanvas => session.reset_viewport(),
        Command::CycleTheme => hotkeys::cycle_theme(session),
        Command::Help => hotkeys::open_help(session, gpu),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn palette_ids_are_unique_url_safe_and_dispatch() {
        let mut ids = HashSet::new();
        let mut commands = HashSet::new();
        for c in PALETTE {
            assert!(ids.insert(c.id), "duplicate id {}", c.id);
            assert!(commands.insert(c.command), "{:?} listed twice", c.command);
            assert!(
                !c.id.is_empty()
                    && c.id.len() <= 40
                    && c.id
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "id {:?} would not survive palette://run parsing",
                c.id
            );
            // The id the page sends comes back as the very same command.
            assert_eq!(Command::from_id(c.id), Some(c.command), "{}", c.id);
            assert_eq!(c.command.palette_entry().map(|e| e.id), Some(c.id));
            let parsed = cef_bridge_parse(&format!("palette://run/{}", c.id));
            assert_eq!(parsed.as_deref(), Some(c.id));
            assert!(!c.title.is_empty());
        }
        assert_eq!(Command::from_id("nope"), None);
        assert_eq!(Command::from_id(""), None);
        // Opening the palette from the palette makes no sense.
        assert!(Command::Palette.palette_entry().is_none());
    }

    /// The id as the navigation intercept hands it to the compositor.
    fn cef_bridge_parse(url: &str) -> Option<String> {
        match cef_bridge::parse_palette_action(url)? {
            cef_bridge::PaletteAction::Run(id) => Some(id),
            _ => None,
        }
    }

    #[test]
    fn the_task_list_is_all_there() {
        for command in [
            Command::NewPage,
            Command::ReopenClosed,
            Command::Settings,
            Command::Bookmarks,
            Command::History,
            Command::Downloads,
            Command::Passwords,
            Command::LockVault,
            Command::Workspaces,
            Command::NewWorkspace,
            Command::Scripts,
            Command::CycleTheme,
            Command::AutoLayout,
            Command::ResetCanvas,
            Command::ZoomToCanvas,
            Command::ReaderMode,
            Command::FindInPage,
            Command::CopyUrl,
            Command::OpenInSystemBrowser,
            Command::Help,
        ] {
            assert!(
                command.palette_entry().is_some(),
                "{command:?} not in palette"
            );
        }
        assert!(PALETTE.iter().any(|c| c.featured));
    }
}
