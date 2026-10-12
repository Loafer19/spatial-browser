// Canvas-level shortcuts (never forwarded to page content): a key maps to a
// `commands::Command` (`command_for`), run by `commands::run` — the same
// path the Ctrl+K palette uses. The helpers below do the actual work; HTML
// for list pages lives in pages/; pan/zoom mouse gestures are in app.rs.

use crate::browser;
use crate::clipboard_bridge;
use crate::commands::{self, Command};
use crate::output::GpuState;
use crate::pages;
use crate::persistence::bookmarks::{self, Bookmark};
use crate::persistence::downloads::DownloadRecord;
use crate::persistence::history::HistoryEntry;
use crate::persistence::settings::AppSettings;
use crate::persistence::vault::{self, VaultSession};
use crate::persistence::workspaces::WorkspaceStore;
use crate::reader_mode;
use crate::session::Session;
use crate::userscripts::UserScript;
use crate::userstyles::UserStyle;
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};
use winit::event::ElementState;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

/// Handle canvas shortcuts; `true` = do not forward to the page.
pub fn handle(
    event: &winit::event::KeyEvent,
    modifiers: ModifiersState,
    session: &mut Session,
    gpu: &GpuState,
    mut stores: crate::pending_actions::Stores<'_>,
) -> bool {
    if event.state != ElementState::Pressed {
        return false;
    }
    let PhysicalKey::Code(code) = event.physical_key else {
        return false;
    };
    // Ctrl+C/X fall through to the page copy-bridge, which may then write
    // the clipboard (see cef_bridge::clipboard::copy_allowed).
    if modifiers.control_key()
        && !modifiers.shift_key()
        && matches!(code, KeyCode::KeyC | KeyCode::KeyX)
    {
        cef_bridge::clipboard::note_copy_shortcut();
        return false;
    }
    let Some(command) = command_for(
        code,
        modifiers.control_key(),
        modifiers.shift_key(),
        modifiers.alt_key(),
    ) else {
        return false;
    };
    commands::run(command, session, gpu, &mut stores);
    true
}

/// The command a key chord runs (`None` = the page gets the key).
pub(crate) fn command_for(code: KeyCode, ctrl: bool, shift: bool, alt: bool) -> Option<Command> {
    if code == KeyCode::F1 {
        return Some(Command::Help);
    }
    if code == KeyCode::F3 {
        return Some(Command::PerfOverlay);
    }
    // Alt+Left/Right back/forward (separate from Ctrl+ bindings).
    if alt && !ctrl {
        return match code {
            KeyCode::ArrowLeft => Some(Command::Back),
            KeyCode::ArrowRight => Some(Command::Forward),
            _ => None,
        };
    }
    if !ctrl {
        return None;
    }
    // Ctrl+1..9 — switch to workspace slot N.
    // Ctrl+Shift+1..9 — move focused page to slot N (create empty if missing) and follow.
    let slot = match code {
        KeyCode::Digit1 => Some(1),
        KeyCode::Digit2 => Some(2),
        KeyCode::Digit3 => Some(3),
        KeyCode::Digit4 => Some(4),
        KeyCode::Digit5 => Some(5),
        KeyCode::Digit6 => Some(6),
        KeyCode::Digit7 => Some(7),
        KeyCode::Digit8 => Some(8),
        KeyCode::Digit9 => Some(9),
        _ => None,
    };
    if let Some(slot) = slot {
        return Some(if shift {
            Command::MoveToWorkspace(slot)
        } else {
            Command::SwitchWorkspace(slot)
        });
    }
    let pick = |plain: Command, shifted: Command| Some(if shift { shifted } else { plain });
    match code {
        // --- Pages ---
        KeyCode::KeyO if shift => Some(Command::OpenInSystemBrowser),
        KeyCode::KeyF if !shift => Some(Command::FindInPage),
        KeyCode::KeyT => pick(Command::NewPage, Command::ReopenClosed),
        // Ctrl+W close / Ctrl+Shift+W workspaces.
        KeyCode::KeyW => pick(Command::ClosePage, Command::Workspaces),
        // Ctrl+N — new live workspace slot (same as HUD '+').
        KeyCode::KeyN if !shift => Some(Command::NewWorkspace),
        // Ctrl+R reload / Ctrl+Shift+R reader mode.
        KeyCode::KeyR => pick(Command::Reload, Command::ReaderMode),
        KeyCode::KeyV => Some(Command::Paste),
        KeyCode::KeyC if shift => Some(Command::CopyUrl),
        // Page content zoom (CEF zoom_level), not Ctrl+Space canvas zoom.
        KeyCode::Equal => Some(Command::ZoomIn),
        KeyCode::Minus => Some(Command::ZoomOut),
        // Ctrl+0 reset page zoom / Ctrl+Shift+0 reset canvas view.
        KeyCode::Digit0 => pick(Command::ZoomReset, Command::ResetCanvas),

        // --- Lists ---
        KeyCode::KeyD => Some(Command::BookmarkPage),
        KeyCode::KeyB => Some(Command::Bookmarks),
        KeyCode::KeyJ => Some(Command::Downloads),
        KeyCode::KeyH => Some(Command::History),
        KeyCode::KeyK => Some(Command::Palette),
        KeyCode::Slash => Some(Command::Help),
        KeyCode::Comma => Some(Command::Settings),
        KeyCode::KeyU if shift => Some(Command::Scripts),
        KeyCode::KeyP if shift => Some(Command::Passwords),
        KeyCode::KeyL if shift => Some(Command::LockVault),

        // --- Canvas ---
        KeyCode::KeyG => Some(Command::AutoLayout),
        KeyCode::Tab => pick(Command::NextPage, Command::PreviousPage),
        // Ctrl+Space zoom-to-canvas / Ctrl+Shift+Space cycle theme.
        KeyCode::Space => pick(Command::ZoomToCanvas, Command::CycleTheme),

        _ => None,
    }
}

// --- Pages ---

/// Open ephemeral omnibox (submit closes+respawns at destination).
pub(crate) fn open_new(
    session: &mut Session,
    gpu: &GpuState,
    typed_history: &[String],
    settings: &AppSettings,
    history: &[HistoryEntry],
    bookmarks: &[Bookmark],
) {
    let size = gpu.window.inner_size();
    let rect = session.cascade_rect((size.width as f32, size.height as f32));
    let data = pages::omnibox::StartData {
        typed_history,
        history,
        bookmarks,
    };
    let url = pages::omnibox::page_url(&session.theme(), &data, &settings.default_search_engine);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Ctrl+F: find bar above the topmost content page (refocus if open).
pub(crate) fn open_find(session: &mut Session, gpu: &GpuState) {
    if let Some(bar_id) = session.find().map(|f| f.bar_id) {
        if let Some(index) = session
            .pages()
            .iter()
            .position(|p| p.browser.identifier() == bar_id)
        {
            session.bring_to_front(index);
            return;
        }
    }
    let Some(target) = session.pages().iter().rev().find(|p| !p.ephemeral) else {
        return;
    };
    let target_id = target.browser.identifier();
    let rect = crate::session::find_bar_rect(target.rect, gpu.window.scale_factor());
    let bar = browser::spawn(
        gpu,
        &gpu.window,
        &pages::find_bar::page_url(&session.theme()),
        rect,
        true,
    );
    let bar_id = bar.browser.identifier();
    session.add_page(bar);
    session.set_find(Some(crate::session::FindBar {
        bar_id,
        target_id,
        last_query: None,
    }));
}

/// Reopen most recently closed page at its former rect.
pub(crate) fn reopen_closed(session: &mut Session, gpu: &GpuState) {
    if let Some((rect, url)) = session.pop_closed() {
        session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, false));
    }
}

/// Ctrl+Shift+O: the topmost content page in the desktop's browser.
pub(crate) fn open_focused_externally(session: &Session) {
    if let Some(page) = session.pages().iter().rev().find(|p| !p.ephemeral) {
        crate::external::open_in_system_browser(&page.url());
    }
}

pub(crate) fn reload_focused(session: &Session) {
    if let Some(page) = session.pages().last() {
        page.browser.reload();
    }
}

/// Ctrl+V: async wl-paste; text lands via clipboard_bridge::deliver_paste.
pub(crate) fn paste_into_focused(session: &Session) {
    if let Some(page) = session.pages().last() {
        clipboard_bridge::start_paste(page.browser.identifier());
    }
}

/// Ctrl+Shift+C: copy focused page URL via wl-copy.
pub(crate) fn copy_focused_url(session: &Session) {
    let Some(page) = session.pages().last() else {
        return;
    };
    cef_bridge::clipboard::copy_text(&page.url(), false);
}

/// Ctrl+Shift+R: toggle reader mode (off = reload).
pub(crate) fn toggle_reader_mode(session: &Session, settings: &AppSettings) {
    let Some(page) = session.pages().last() else {
        return;
    };
    if page.reader_mode.get() {
        page.browser.reload();
        page.reader_mode.set(false);
        return;
    }
    let theme = reader_mode::READER_THEMES
        .get(settings.reader_theme)
        .unwrap_or(&reader_mode::READER_THEMES[0]);
    if let Some(frame) = page.browser.main_frame() {
        frame.execute_java_script(
            Some(&reader_mode::extract_script(theme).as_str().into()),
            Some(&"".into()),
            0,
        );
        page.reader_mode.set(true);
    }
}

pub(crate) fn go_back(session: &Session) {
    if let Some(page) = session.pages().last() {
        if page.browser.can_go_back() != 0 {
            page.browser.go_back();
        }
    }
}

pub(crate) fn go_forward(session: &Session) {
    if let Some(page) = session.pages().last() {
        if page.browser.can_go_forward() != 0 {
            page.browser.go_forward();
        }
    }
}

pub(crate) fn page_zoom(session: &Session, command: cef::ZoomCommand) {
    if let Some(page) = session.pages().last() {
        if let Some(host) = page.browser.host() {
            host.zoom(command);
        }
    }
}

// --- Lists ---

/// Bookmark focused page URL (refuses ephemeral built-in pages).
pub(crate) fn bookmark_focused(session: &Session, bookmarks: &mut Vec<Bookmark>) {
    let Some(page) = session.pages().last() else {
        return;
    };
    if page.ephemeral {
        return;
    }
    let Some(url) = page
        .browser
        .main_frame()
        .map(|frame| cef::CefString::from(&frame.url()).to_string())
    else {
        return;
    };
    if bookmarks.iter().any(|b| b.url == url) {
        return;
    }
    bookmarks.push(Bookmark {
        url,
        title: None,
        folder: None,
    });
    bookmarks::save(bookmarks);
}

pub(crate) fn open_bookmarks(session: &mut Session, gpu: &GpuState, bookmarks: &[Bookmark]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::bookmarks_list::page_url(&session.theme(), bookmarks, None);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+J downloads list.
pub(crate) fn open_downloads(session: &mut Session, gpu: &GpuState, downloads: &[DownloadRecord]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::downloads_list::page_url(
        &session.theme(),
        downloads,
        &cef_bridge::active_downloads(),
    );
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+H history list.
pub(crate) fn open_history(session: &mut Session, gpu: &GpuState, history: &[HistoryEntry]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::history_list::page_url(&session.theme(), history);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+Shift+W workspace list.
pub(crate) fn open_workspaces(session: &mut Session, gpu: &GpuState, workspaces: &WorkspaceStore) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::workspace_list::page_url(&session.theme(), workspaces);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+, settings page.
pub(crate) fn open_settings(session: &mut Session, gpu: &GpuState, settings: &AppSettings) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::settings_list::page_url(&session.theme(), settings);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+Shift+U userscripts + userstyles list.
pub(crate) fn open_userscripts(
    session: &mut Session,
    gpu: &GpuState,
    scripts: &[UserScript],
    styles: &[UserStyle],
) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::userscripts_list::page_url(&session.theme(), scripts, styles);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

pub(crate) fn open_passwords(session: &mut Session, gpu: &GpuState, vault: Option<&VaultSession>) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.52).clamp(420.0, 720.0);
    let h = (size.height as f32 * 0.75).clamp(420.0, 800.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = match vault {
        Some(v) => {
            v.touch();
            pages::passwords_list::page_url(
                &session.theme(),
                &v.data.entries,
                &v.data.never_save,
                None,
                None,
                "saved",
            )
        }
        None => pages::passwords_list::unlock_url(&session.theme(), !vault::exists(), None),
    };
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Ctrl+K: open the command palette over the focused tile (which its
/// commands then act on), or close it when it is already open.
pub(crate) fn toggle_palette(
    session: &mut Session,
    gpu: &GpuState,
    bookmarks: &[Bookmark],
    history: &[HistoryEntry],
    settings: &AppSettings,
) {
    if crate::pending_actions::close_palette(session) {
        return;
    }
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(420.0, 680.0);
    let h = (size.height as f32 * 0.62).clamp(380.0, 600.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let target = session.pages().last().map(|p| p.browser.identifier());
    // Topmost first: the page you were just on leads. Ephemeral utility
    // tiles (help, lists) aren't something worth switching *to*.
    let open: Vec<pages::palette::OpenPage> = session
        .pages()
        .iter()
        .rev()
        .filter(|p| !p.ephemeral)
        .map(|p| {
            let id = p.browser.identifier();
            pages::palette::OpenPage {
                id,
                url: p.url(),
                title: crate::pending_actions::page_title(id),
                current: Some(id) == target,
            }
        })
        .collect();
    let url = pages::palette::page_url(
        &session.theme(),
        &open,
        bookmarks,
        history,
        &settings.default_search_engine,
    );
    let palette = browser::spawn(gpu, &gpu.window, &url, rect, true);
    crate::pending_actions::palette_opened(palette.browser.identifier(), target);
    session.add_page(palette);
}

// --- Canvas ---

/// Grid-layout open pages to fill the window.
pub(crate) fn auto_layout(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    session.auto_layout(
        (size.width as f32, size.height as f32),
        gpu.window.scale_factor(),
    );
}

/// Ctrl+Space: zoom the focused page to fill the window (toggle).
pub(crate) fn zoom_to_canvas(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    session.toggle_zoom_focused(
        (size.width as f32, size.height as f32),
        gpu.window.scale_factor(),
    );
}

// --- Other ---

/// Ctrl+Shift+Space: next UI theme. Built-in pages pick the new palette
/// up in place (ui.js).
pub(crate) fn cycle_theme(session: &mut Session) {
    session.cycle_theme();
    let js = pages::ui::theme_js(&session.theme());
    for page in session.pages() {
        if pages::kind_of(&page.url()).is_some() {
            pages::ui::run_js(&page.browser, &js);
        }
    }
}

pub(crate) fn open_help(session: &mut Session, gpu: &GpuState) {
    let rect = centered_tile(session, gpu);
    let url = pages::help::page_url(&session.theme());
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

fn centered_tile(session: &Session, gpu: &GpuState) -> crate::output::Rect {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.6).clamp(420.0, 720.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    session.centered_rect((size.width as f32, size.height as f32), (w, h))
}

pub(crate) fn open_about(session: &mut Session, gpu: &GpuState) {
    let rect = centered_tile(session, gpu);
    let url = pages::about::page_url(&session.theme());
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Site info for the topmost web page (http/https only).
pub(crate) fn open_site_info(
    session: &mut Session,
    gpu: &GpuState,
    s: &crate::pending_actions::Stores<'_>,
) {
    let Some(url) = session
        .pages()
        .iter()
        .rev()
        .find(|p| !p.ephemeral)
        .map(|p| p.url())
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
    else {
        log::info!("site info: no web page focused");
        return;
    };
    let rect = centered_tile(session, gpu);
    let url = pages::site_info::page_url(
        &session.theme(),
        &url,
        &crate::pending_actions::site_context(s),
    );
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Ctrl+Shift+T` → (key, ctrl, shift, alt), as the help text spells it.
    fn chord(shortcut: &str) -> (KeyCode, bool, bool, bool) {
        let mut parts: Vec<&str> = shortcut.split('+').collect();
        let key = parts.pop().expect("a key");
        let has = |m: &str| parts.contains(&m);
        let code = match key {
            "," => KeyCode::Comma,
            "/" => KeyCode::Slash,
            "0" => KeyCode::Digit0,
            "Space" => KeyCode::Space,
            "Tab" => KeyCode::Tab,
            "Left" => KeyCode::ArrowLeft,
            "Right" => KeyCode::ArrowRight,
            "F1" => KeyCode::F1,
            "F3" => KeyCode::F3,
            letter => {
                const LETTERS: [KeyCode; 26] = [
                    KeyCode::KeyA,
                    KeyCode::KeyB,
                    KeyCode::KeyC,
                    KeyCode::KeyD,
                    KeyCode::KeyE,
                    KeyCode::KeyF,
                    KeyCode::KeyG,
                    KeyCode::KeyH,
                    KeyCode::KeyI,
                    KeyCode::KeyJ,
                    KeyCode::KeyK,
                    KeyCode::KeyL,
                    KeyCode::KeyM,
                    KeyCode::KeyN,
                    KeyCode::KeyO,
                    KeyCode::KeyP,
                    KeyCode::KeyQ,
                    KeyCode::KeyR,
                    KeyCode::KeyS,
                    KeyCode::KeyT,
                    KeyCode::KeyU,
                    KeyCode::KeyV,
                    KeyCode::KeyW,
                    KeyCode::KeyX,
                    KeyCode::KeyY,
                    KeyCode::KeyZ,
                ];
                let c = letter.as_bytes();
                assert!(
                    c.len() == 1 && c[0].is_ascii_uppercase(),
                    "unknown key {letter:?} in {shortcut:?}"
                );
                LETTERS[usize::from(c[0] - b'A')]
            }
        };
        (code, has("Ctrl"), has("Shift"), has("Alt"))
    }

    #[test]
    fn every_shortcut_the_palette_shows_runs_that_command() {
        for entry in commands::PALETTE {
            let Some(shortcut) = entry.shortcut else {
                continue;
            };
            let (code, ctrl, shift, alt) = chord(shortcut);
            assert_eq!(
                command_for(code, ctrl, shift, alt),
                Some(entry.command),
                "{shortcut} for {}",
                entry.id
            );
        }
    }

    #[test]
    fn key_map() {
        assert_eq!(
            command_for(KeyCode::KeyK, true, false, false),
            Some(Command::Palette)
        );
        assert_eq!(
            command_for(KeyCode::F1, false, false, false),
            Some(Command::Help)
        );
        assert_eq!(
            command_for(KeyCode::Digit3, true, false, false),
            Some(Command::SwitchWorkspace(3))
        );
        assert_eq!(
            command_for(KeyCode::Digit9, true, true, false),
            Some(Command::MoveToWorkspace(9))
        );
        assert_eq!(
            command_for(KeyCode::Tab, true, true, false),
            Some(Command::PreviousPage)
        );
        // Plain typing, Ctrl+C (the page's copy bridge), Ctrl+Shift+F and
        // Alt+letters stay with the page.
        assert_eq!(command_for(KeyCode::KeyK, false, false, false), None);
        assert_eq!(command_for(KeyCode::KeyC, true, false, false), None);
        assert_eq!(command_for(KeyCode::KeyF, true, true, false), None);
        assert_eq!(command_for(KeyCode::KeyN, true, true, false), None);
        assert_eq!(command_for(KeyCode::KeyD, false, false, true), None);
    }
}
