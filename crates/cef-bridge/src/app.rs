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
            // notifies ReadAnythingSoftNavigationObserver. That observer
            // calls tabs::TabInterface::GetFromContents(), which
            // dereferences internally before its own `if (!tab) return;`
            // guard can run — null-derefs on any WebContents that isn't a
            // real browser tab, i.e. every WebContents in a windowless/
            // Alloy-style embedding, on any SPA-style client navigation
            // (confirmed: YouTube, Google Images' lightbox). Upstream:
            // https://github.com/chromiumembedded/cef/issues/4234 (fixed
            // for M152; cef-rs has no 152 track yet — we're on 151.8.0).
            //
            // The observer is gated solely on
            // features::IsImmersiveReadAnythingEnabled(), so disabling
            // that feature skips OnSoftNavigation's body entirely before
            // it reaches the crashing call. (Two earlier attempts here —
            // disable-features=ReadAnything,SoftNavigationHeuristics and
            // disable-blink-features=SoftNavigationHeuristics,
            // SoftNavigationDetection — targeted the wrong feature names
            // and didn't help.) Reading Mode is browser-chrome UI a
            // windowless app can't surface anyway, so this costs nothing.
            //
            // Prefer software decode over broken VAAPI paths on Linux
            // (`vaEndPicture failed` in logs). Do **not** also set
            // disable-accelerated-video-decode: some CEF builds only
            // expose H.264/AAC via the accelerated path, and killing
            // that entirely makes Twitch report Error #4000.
            command_line.append_switch_with_value(
                Some(&"disable-features".into()),
                Some(
                    &"ImmersiveReadAnything,VaapiVideoDecoder,VaapiVideoDecodeLinuxGL,VaapiVideoEncoder"
                        .into(),
                ),
            );
            // Embedded OSR has no reliable "user gesture" for media;
            // without this, sites that gate MSE/autoplay can refuse play.
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
            // GPU/renderer children need the same media flags as the
            // browser process — otherwise Twitch still hits broken HW decode.
            command_line.append_switch_with_value(
                Some(&"disable-features".into()),
                Some(
                    &"ImmersiveReadAnything,VaapiVideoDecoder,VaapiVideoDecodeLinuxGL,VaapiVideoEncoder"
                        .into(),
                ),
            );
            // Hard stop: feature flags alone were not enough (vaEndPicture
            // still appeared). Force software decode in GPU/renderer.
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
