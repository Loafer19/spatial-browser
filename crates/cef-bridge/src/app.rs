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
            // ImmersiveReadAnything: SPA soft-nav null-deref in Alloy/OSR
            // (CEF#4234), fixed in M152; still off since OSR has no reading
            // mode UI to lose. Disable VAAPI — unreliable
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

        // Every process (browser, renderer, …) must see the same scheme list.
        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            if let Some(registrar) = registrar {
                crate::spatial::register_scheme(registrar);
            }
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
            crate::spatial::register_handler_factory();
        }

        // external_message_pump: tell the embedder loop when to call
        // do_message_loop_work (may be called from any thread).
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            crate::message_pump::schedule(delay_ms);
        }

        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            let Some(command_line) = command_line else {
                return;
            };

            // No SOP/TLS relaxations here: pages share one process tree with
            // the vault and the scheme:// action bridge.
            command_line.append_switch(Some(&"disable-session-crashed-bubble".into()));
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
