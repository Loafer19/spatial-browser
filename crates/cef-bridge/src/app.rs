// The process-wide CEF `App`/`BrowserProcessHandler`: command-line
// flags applied to every process (browser and re-exec'd subprocesses
// alike) and to child processes specifically, plus tracking whether
// CEF's context has finished initializing.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

#[derive(Clone)]
pub struct OsrApp {}

impl OsrApp {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for OsrApp {
    fn default() -> Self {
        Self::new()
    }
}

wrap_app! {
    pub struct AppBuilder {
        app: OsrApp,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&cef::CefStringUtf16>,
            command_line: Option<&mut cef::CommandLine>,
        ) {
            let Some(command_line) = command_line else {
                return;
            };

            command_line.append_switch(Some(&"no-startup-window".into()));
            command_line.append_switch(Some(&"noerrdialogs".into()));
            command_line.append_switch(Some(&"hide-crash-restore-bubble".into()));
            command_line.append_switch(Some(&"use-mock-keychain".into()));
            // Chromium's soft-navigation tracking (used for SPA Web
            // Vitals) calls PageLoadTracker::OnSoftNavigation, which
            // notifies every registered observer including
            // ReadAnythingSoftNavigationObserver — that observer assumes
            // every WebContents has a real browser Tab, which a
            // windowless/OSR embedding never has, and null-derefs
            // (TabInterface::GetFromContents) on any SPA-style client
            // navigation (confirmed: YouTube, Google Images' lightbox).
            // Disabling just ReadAnything didn't help — the observer is
            // apparently registered unconditionally regardless of that
            // flag. Disabling SoftNavigationHeuristics via
            // --disable-features (a //base feature) didn't help either,
            // even though chrome://version confirms the switch reaches
            // the process — soft-navigation instrumentation is Blink
            // runtime code, gated through the separate
            // --disable-blink-features namespace, not --disable-features.
            command_line.append_switch_with_value(
                Some(&"disable-features".into()),
                Some(&"ReadAnything,SoftNavigationHeuristics".into()),
            );
            command_line.append_switch_with_value(
                Some(&"disable-blink-features".into()),
                Some(&"SoftNavigationHeuristics,SoftNavigationDetection".into()),
            );
        }

        fn browser_process_handler(&self) -> Option<cef::BrowserProcessHandler> {
            Some(BrowserProcessHandlerBuilder::build(
                OsrBrowserProcessHandler::new(),
            ))
        }
    }
}

impl AppBuilder {
    pub fn build(app: OsrApp) -> cef::App {
        Self::new(app)
    }
}

#[derive(Clone)]
pub struct OsrBrowserProcessHandler {
    is_cef_ready: RefCell<bool>,
}

impl OsrBrowserProcessHandler {
    pub fn new() -> Self {
        Self {
            is_cef_ready: RefCell::new(false),
        }
    }
}

impl Default for OsrBrowserProcessHandler {
    fn default() -> Self {
        Self::new()
    }
}

wrap_browser_process_handler! {
    pub struct BrowserProcessHandlerBuilder {
        handler: OsrBrowserProcessHandler,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            *self.handler.is_cef_ready.borrow_mut() = true;
        }

        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            let Some(command_line) = command_line else {
                return;
            };

            command_line.append_switch(Some(&"disable-web-security".into()));
            command_line.append_switch(Some(&"allow-running-insecure-content".into()));
            command_line.append_switch(Some(&"disable-session-crashed-bubble".into()));
            command_line.append_switch(Some(&"ignore-certificate-errors".into()));
            command_line.append_switch(Some(&"ignore-ssl-errors".into()));
        }
    }
}

impl BrowserProcessHandlerBuilder {
    pub fn build(handler: OsrBrowserProcessHandler) -> BrowserProcessHandler {
        Self::new(handler)
    }
}
