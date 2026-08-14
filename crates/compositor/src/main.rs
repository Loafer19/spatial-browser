// Compositor: owns the window, the GPU surface, and the spatial canvas.
// This step: one CEF page, off-screen rendered into a shared GPU texture
// (no CPU copy), drawn as a single full-window textured quad. Multiple
// pages arranged on a pannable/zoomable canvas is the next step; for now
// there's exactly one quad covering the whole surface.
//
// CEF's multi-process model means this same binary is re-exec'd as the
// renderer/gpu/utility helper processes, so the CEF bootstrap (execute_process
// / initialize) has to run at the very top of main(), before any window or
// wgpu setup, and the winit loop has to cooperatively pump CEF's message
// loop (do_message_loop_work) instead of blocking in `run_app`.

mod input;
mod output;

use cef::{args::Args, *};
use cef_bridge::{
    AppBuilder, ClientBuilder, CURSOR, OsrApp, OsrRenderHandler, OsrRequestContextHandler,
    RequestContextHandlerBuilder,
};
use input::{KeyboardInput, MouseInput};
use output::{FrameOutcome, GpuState};
use std::{cell::RefCell, process::ExitCode, rc::Rc, sync::Arc, thread::sleep, time::Duration};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::pump_events::{EventLoopExtPumpEvents, PumpStatus},
    window::{WindowAttributes, WindowId},
};

const HOME_PAGE: &str = "https://example.com";

struct BrowserState {
    browser: cef::Browser,
    size: Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
}

#[derive(Default)]
struct App {
    state: Option<GpuState>,
    browser: Option<BrowserState>,
    mouse: MouseInput,
    keyboard: KeyboardInput,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(WindowAttributes::default().with_title("spatial-browser"))
                .expect("failed to create window"),
        );
        let state = pollster::block_on(GpuState::new(window.clone()));

        // Shared-texture (GPU) OSR needs the CEF GPU process's DMA-BUF export
        // and our wgpu Vulkan import to land on the same physical GPU. On a
        // hybrid-graphics laptop, Chromium's GPU process defaults to the
        // display-driving iGPU while wgpu's HighPerformance pick can land on
        // the discrete GPU; Chromium also strips most env vars from its
        // child processes, so there's no reliable way to force both onto
        // the same device from here. CPU OSR (on_paint, plain memcpy) has no
        // such cross-device requirement, so that's what's wired up for now.
        let window_info = WindowInfo {
            windowless_rendering_enabled: true as _,
            shared_texture_enabled: false as _,
            external_begin_frame_enabled: true as _,
            ..Default::default()
        };

        let device_scale_factor = window.scale_factor();
        let (render_handler, browser_size) = OsrRenderHandler::new(
            state.device.clone(),
            state.queue.clone(),
            state.texture_bind_group_layout.clone(),
            device_scale_factor as _,
            window.inner_size().to_logical(device_scale_factor),
        );

        let browser_settings = BrowserSettings {
            windowless_frame_rate: 60,
            ..Default::default()
        };
        let mut context = cef::request_context_create_context(
            Some(&RequestContextSettings::default()),
            Some(&mut RequestContextHandlerBuilder::build(
                OsrRequestContextHandler {},
            )),
        );

        let browser = cef::browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut ClientBuilder::build(render_handler)),
            Some(&HOME_PAGE.into()),
            Some(&browser_settings),
            None,
            context.as_mut(),
        )
        .expect("failed to create CEF browser");

        self.browser.replace(BrowserState {
            browser,
            size: browser_size,
        });
        self.state = Some(state);
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        if state.window.id() != id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);
                if let Some(browser) = &self.browser {
                    *browser.size.borrow_mut() = size.to_logical(state.window.scale_factor());
                    if let Some(host) = browser.browser.host() {
                        host.was_resized();
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = state.window.scale_factor();
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse
                    .cursor_moved((position.x, position.y), scale, host.as_ref());
            }
            WindowEvent::CursorLeft { .. } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse.cursor_left(host.as_ref());
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse.button(element_state, button, host.as_ref());
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse.wheel(delta, host.as_ref());
            }
            WindowEvent::Focused(focused) => {
                if let Some(host) = self.browser.as_ref().and_then(|b| b.browser.host()) {
                    host.set_focus(focused as _);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.keyboard.modifiers_changed(modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.keyboard.key_event(&event, host.as_ref());
            }
            WindowEvent::RedrawRequested => {
                if let Some(host) = self.browser.as_ref().and_then(|b| b.browser.host()) {
                    host.send_external_begin_frame();
                }
                if let Some(icon) = CURSOR.with_borrow_mut(|cursor| cursor.take()) {
                    state.window.set_cursor(icon);
                }
                match state.render() {
                    FrameOutcome::Rendered | FrameOutcome::Skip => {}
                    FrameOutcome::Reconfigure => {
                        let size = state.window.inner_size();
                        state.resize(size.width, size.height);
                    }
                    FrameOutcome::Fatal => {
                        log::error!("fatal surface validation error, exiting");
                        event_loop.exit();
                    }
                }
                state.window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let cmd = args.as_cmd_line().unwrap();
    let is_browser_process = cmd.has_switch(Some(&"type".into())) != 1;

    let mut app = AppBuilder::build(OsrApp::new());
    let ret = execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());

    if is_browser_process {
        assert!(ret == -1, "cannot execute browser process");
    } else {
        // Non-browser (renderer/gpu/utility) subprocess: execute_process
        // already ran the subprocess entry point above, nothing left to do.
        return ExitCode::from(0);
    }

    let settings = Settings {
        windowless_rendering_enabled: true as _,
        external_message_pump: true as _,
        ..Default::default()
    };
    assert_eq!(
        initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()),
        1,
        "CEF initialize failed"
    );

    let mut event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    let exit_code = loop {
        do_message_loop_work();
        match event_loop.pump_app_events(Some(Duration::ZERO), &mut app) {
            PumpStatus::Exit(code) => break ExitCode::from(code as u8),
            PumpStatus::Continue => {}
        }
        sleep(Duration::from_millis(1000 / 60));
    };

    cef::shutdown();
    exit_code
}
