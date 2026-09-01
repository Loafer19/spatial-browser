// Canvas-level shortcuts (never forwarded to page content). HTML for list
// pages lives in pages/; pan/zoom mouse gestures are in app.rs.

use crate::browser;
use crate::clipboard_bridge;
use crate::output::GpuState;
use crate::pages;
use crate::persistence::bookmarks::{self, Bookmark};
use crate::persistence::downloads::DownloadRecord;
use crate::persistence::history::HistoryEntry;
use crate::persistence::settings::AppSettings;
use crate::persistence::workspaces::{self, WorkspaceRuntime, WorkspaceStore};
use crate::reader_mode;
use crate::session::Session;
use crate::persistence::vault::{self, VaultSession};
use crate::userscripts::{self, UserScript};
use crate::userstyles::{self, UserStyle};
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};
use winit::event::ElementState;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

/// Handle canvas shortcuts; `true` = do not forward to the page.
pub fn handle(
    event: &winit::event::KeyEvent,
    modifiers: ModifiersState,
    session: &mut Session,
    gpu: &GpuState,
    bookmarks: &mut Vec<Bookmark>,
    typed_history: &[String],
    downloads: &[DownloadRecord],
    history: &[HistoryEntry],
    workspaces: &mut WorkspaceStore,
    workspace_runtime: &mut WorkspaceRuntime,
    settings: &AppSettings,
    userscripts: &mut Vec<UserScript>,
    userstyles: &mut Vec<UserStyle>,
    vault: &mut Option<VaultSession>,
) -> bool {
    if event.state != ElementState::Pressed {
        return false;
    }

    if event.physical_key == PhysicalKey::Code(KeyCode::F1) {
        open_help(session, gpu);
        return true;
    }

    // Alt+Left/Right back/forward (separate from Ctrl+ bindings).
    if modifiers.alt_key() && !modifiers.control_key() {
        match event.physical_key {
            PhysicalKey::Code(KeyCode::ArrowLeft) => {
                go_back(session);
                return true;
            }
            PhysicalKey::Code(KeyCode::ArrowRight) => {
                go_forward(session);
                return true;
            }
            _ => {}
        }
    }

    if !modifiers.control_key() {
        return false;
    }

    match event.physical_key {
        // --- Pages ---
        PhysicalKey::Code(KeyCode::KeyT) => {
            if modifiers.shift_key() {
                reopen_closed(session, gpu);
            } else {
                open_new(session, gpu, typed_history, settings);
            }
            true
        }
        // Ctrl+W close / Ctrl+Shift+W workspaces.
        PhysicalKey::Code(KeyCode::KeyW) => {
            if modifiers.shift_key() {
                open_workspaces(session, gpu, workspaces);
            } else {
                session.close_topmost();
            }
            true
        }
        // Ctrl+N — new live workspace slot (same as HUD '+').
        PhysicalKey::Code(KeyCode::KeyN) => {
            if !modifiers.shift_key() {
                workspaces::add_and_switch(workspaces, workspace_runtime, session, gpu);
                true
            } else {
                false
            }
        }
        // Ctrl+1..9 — switch to workspace slot N.
        PhysicalKey::Code(
            code @ (KeyCode::Digit1
                | KeyCode::Digit2
                | KeyCode::Digit3
                | KeyCode::Digit4
                | KeyCode::Digit5
                | KeyCode::Digit6
                | KeyCode::Digit7
                | KeyCode::Digit8
                | KeyCode::Digit9),
        ) if !modifiers.shift_key() => {
            let id = match code {
                KeyCode::Digit1 => 1,
                KeyCode::Digit2 => 2,
                KeyCode::Digit3 => 3,
                KeyCode::Digit4 => 4,
                KeyCode::Digit5 => 5,
                KeyCode::Digit6 => 6,
                KeyCode::Digit7 => 7,
                KeyCode::Digit8 => 8,
                KeyCode::Digit9 => 9,
                _ => 0,
            };
            if id > 0 {
                workspaces::switch_to(workspaces, workspace_runtime, session, gpu, id);
            }
            true
        }
        // Ctrl+R reload / Ctrl+Shift+R reader mode.
        PhysicalKey::Code(KeyCode::KeyR) => {
            if modifiers.shift_key() {
                toggle_reader_mode(session, settings);
            } else {
                reload_focused(session);
            }
            true
        }
        PhysicalKey::Code(KeyCode::KeyV) => {
            paste_into_focused(session);
            true
        }
        // Ctrl+C falls through to page copy-bridge; Ctrl+Shift+C = copy URL.
        PhysicalKey::Code(KeyCode::KeyC) => {
            if modifiers.shift_key() {
                copy_focused_url(session);
                true
            } else {
                false
            }
        }
        // Page content zoom (CEF zoom_level), not Ctrl+Space canvas zoom.
        PhysicalKey::Code(KeyCode::Equal) => {
            page_zoom(session, cef::ZoomCommand::IN);
            true
        }
        PhysicalKey::Code(KeyCode::Minus) => {
            page_zoom(session, cef::ZoomCommand::OUT);
            true
        }
        // Ctrl+0 reset page zoom / Ctrl+Shift+0 reset canvas view.
        PhysicalKey::Code(KeyCode::Digit0) => {
            if modifiers.shift_key() {
                session.reset_viewport();
            } else {
                page_zoom(session, cef::ZoomCommand::RESET);
            }
            true
        }

        // --- Lists ---
        PhysicalKey::Code(KeyCode::KeyD) => {
            bookmark_focused(session, bookmarks);
            true
        }
        PhysicalKey::Code(KeyCode::KeyB) => {
            open_bookmarks(session, gpu, bookmarks);
            true
        }
        PhysicalKey::Code(KeyCode::KeyJ) => {
            open_downloads(session, gpu, downloads);
            true
        }
        PhysicalKey::Code(KeyCode::KeyH) => {
            open_history(session, gpu, history);
            true
        }
        PhysicalKey::Code(KeyCode::KeyK) => {
            open_switcher(session, gpu);
            true
        }
        PhysicalKey::Code(KeyCode::Slash) => {
            open_help(session, gpu);
            true
        }
        PhysicalKey::Code(KeyCode::Comma) => {
            open_settings(session, gpu, settings);
            true
        }
        // Opening reloads from disk so new files appear without restart.
        PhysicalKey::Code(KeyCode::KeyU) if modifiers.shift_key() => {
            userscripts::reload(userscripts);
            userstyles::reload(userstyles);
            open_userscripts(session, gpu, userscripts, userstyles);
            true
        }
        PhysicalKey::Code(KeyCode::KeyP) if modifiers.shift_key() => {
            open_passwords(session, gpu, vault.as_ref());
            true
        }

        // --- Canvas ---
        PhysicalKey::Code(KeyCode::KeyG) => {
            auto_layout(session, gpu);
            true
        }
        PhysicalKey::Code(KeyCode::Tab) => {
            session.rotate_focus(modifiers.shift_key());
            true
        }
        // Ctrl+Space zoom-to-canvas / Ctrl+Shift+Space cycle theme.
        PhysicalKey::Code(KeyCode::Space) => {
            if modifiers.shift_key() {
                session.cycle_theme();
            } else {
                let size = gpu.window.inner_size();
                session.toggle_zoom_focused(
                    (size.width as f32, size.height as f32),
                    gpu.window.scale_factor(),
                );
            }
            true
        }

        _ => false,
    }
}

// --- Pages ---

/// Open ephemeral omnibox (submit closes+respawns at destination).
pub(crate) fn open_new(
    session: &mut Session,
    gpu: &GpuState,
    typed_history: &[String],
    settings: &AppSettings,
) {
    let size = gpu.window.inner_size();
    let rect = session.cascade_rect((size.width as f32, size.height as f32));
    let url = pages::omnibox::page_url(
        &session.theme(),
        typed_history,
        &settings.default_search_engine,
    );
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Reopen most recently closed page at its former rect.
fn reopen_closed(session: &mut Session, gpu: &GpuState) {
    if let Some((rect, url)) = session.pop_closed() {
        session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, false));
    }
}

fn reload_focused(session: &Session) {
    if let Some(page) = session.pages().last() {
        page.browser.reload();
    }
}

/// Ctrl+V: wl-paste + execCommand('insertText') (OSR has no CEF clipboard).
fn paste_into_focused(session: &Session) {
    let Some(page) = session.pages().last() else {
        return;
    };
    let Ok(output) = std::process::Command::new("wl-paste")
        .arg("--no-newline")
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return; // e.g. clipboard empty — wl-paste exits non-zero
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if text.is_empty() {
        return;
    }
    if let Some(frame) = page.browser.main_frame() {
        let script = format!(
            "document.execCommand('insertText', false, {});",
            clipboard_bridge::js_literal(&text)
        );
        frame.execute_java_script(Some(&script.as_str().into()), Some(&"".into()), 0);
    }
}

/// Ctrl+Shift+C: copy focused page URL via wl-copy.
fn copy_focused_url(session: &Session) {
    let Some(page) = session.pages().last() else {
        return;
    };
    let url = page.url();
    if let Err(e) = std::process::Command::new("wl-copy").arg(&url).spawn() {
        log::warn!("wl-copy failed (couldn't copy page URL): {e}");
    }
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

fn go_back(session: &Session) {
    if let Some(page) = session.pages().last() {
        if page.browser.can_go_back() != 0 {
            page.browser.go_back();
        }
    }
}

fn go_forward(session: &Session) {
    if let Some(page) = session.pages().last() {
        if page.browser.can_go_forward() != 0 {
            page.browser.go_forward();
        }
    }
}

fn page_zoom(session: &Session, command: cef::ZoomCommand) {
    if let Some(page) = session.pages().last() {
        if let Some(host) = page.browser.host() {
            host.zoom(command);
        }
    }
}

// --- Lists ---

/// Bookmark focused page URL (refuses ephemeral data: pages).
fn bookmark_focused(session: &Session, bookmarks: &mut Vec<Bookmark>) {
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
fn open_downloads(session: &mut Session, gpu: &GpuState, downloads: &[DownloadRecord]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::downloads_list::page_url(&session.theme(), downloads);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+H history list.
fn open_history(session: &mut Session, gpu: &GpuState, history: &[HistoryEntry]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::history_list::page_url(&session.theme(), history);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+Shift+W workspace list.
fn open_workspaces(session: &mut Session, gpu: &GpuState, workspaces: &WorkspaceStore) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::workspace_list::page_url(&session.theme(), workspaces);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+, settings page.
fn open_settings(session: &mut Session, gpu: &GpuState, settings: &AppSettings) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::settings_list::page_url(&session.theme(), settings);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+Shift+U userscripts + userstyles list.
fn open_userscripts(
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

pub(crate) fn open_passwords(
    session: &mut Session,
    gpu: &GpuState,
    vault: Option<&VaultSession>,
) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.52).clamp(420.0, 720.0);
    let h = (size.height as f32 * 0.75).clamp(420.0, 800.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = match vault {
        Some(v) => pages::passwords_list::page_url(
            &session.theme(),
            &v.data.entries,
            &v.data.never_save,
            None,
            None,
            "saved",
        ),
        None => pages::passwords_list::unlock_url(&session.theme(), !vault::exists(), None),
    };
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Open Ctrl+K switcher (non-ephemeral pages only).
fn open_switcher(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.6).clamp(360.0, 640.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let entries: Vec<(i32, String)> = session
        .pages()
        .iter()
        .filter(|p| !p.ephemeral)
        .map(|p| (p.browser.identifier(), p.url()))
        .collect();
    let url = pages::switcher::page_url(&session.theme(), &entries);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

// --- Canvas ---

/// Grid-layout open pages to fill the window.
fn auto_layout(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    session.auto_layout(
        (size.width as f32, size.height as f32),
        gpu.window.scale_factor(),
    );
}

// --- Other ---

fn open_help(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.6).clamp(420.0, 720.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::help::page_url(&session.theme());
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}
