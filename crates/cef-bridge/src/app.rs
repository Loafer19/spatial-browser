// Process-wide CEF App / BrowserProcessHandler: shared command-line flags
// and context-initialized tracking.

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
            // Disable ImmersiveReadAnything: SPA soft-nav crashes Alloy/OSR
            // (CEF#4234, fixed M152; we're on 151). Disable VAAPI — unreliable
            // in this OSR/hybrid-GPU setup; flags alone still open libva.
            command_line.append_switch_with_value(
                Some(&"disable-features".into()),
                Some(
                    &"ImmersiveReadAnything,VaapiVideoDecoder,VaapiVideoDecodeLinuxGL,VaapiVideoEncoder,VaapiIgnoreDriverChecks,AcceleratedVideoDecodeLinuxGL,AcceleratedVideoDecodeLinuxZeroCopyGL"
                        .into(),
                ),
            );
            command_line.append_switch(Some(&"disable-accelerated-video-decode".into()));
            command_line.append_switch(Some(&"disable-accelerated-video-encode".into()));
            // OSR has no reliable user-gesture for media autoplay.
            command_line.append_switch_with_value(
                Some(&"autoplay-policy".into()),
                Some(&"no-user-gesture-required".into()),
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
            // Same media flags as browser process (VAAPI is in the GPU process).
            command_line.append_switch_with_value(
                Some(&"disable-features".into()),
                Some(
                    &"ImmersiveReadAnything,VaapiVideoDecoder,VaapiVideoDecodeLinuxGL,VaapiVideoEncoder,VaapiIgnoreDriverChecks,AcceleratedVideoDecodeLinuxGL,AcceleratedVideoDecodeLinuxZeroCopyGL"
                        .into(),
                ),
            );
            command_line.append_switch(Some(&"disable-accelerated-video-decode".into()));
            command_line.append_switch(Some(&"disable-accelerated-video-encode".into()));
            command_line.append_switch_with_value(
                Some(&"autoplay-policy".into()),
                Some(&"no-user-gesture-required".into()),
            );
        }
    }
}

impl BrowserProcessHandlerBuilder {
    pub fn build(handler: OsrBrowserProcessHandler) -> BrowserProcessHandler {
        Self::new(handler)
    }
}
