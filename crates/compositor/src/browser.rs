// Spawning a CEF browser instance against our own GpuState (device/queue/
// texture bind group layout) and window. One browser for now — this is
// where per-page browser creation grows into when the canvas holds more
// than one page.

use crate::output::GpuState;
use cef_bridge::{
    ClientBuilder, OsrRenderHandler, OsrRequestContextHandler, RequestContextHandlerBuilder,
};
use std::{cell::RefCell, rc::Rc};
use winit::window::Window;

pub struct BrowserState {
    pub browser: cef::Browser,
    pub size: Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
}

pub fn spawn(state: &GpuState, window: &Window, home_page: &str) -> BrowserState {
    // Shared-texture (GPU) OSR needs the CEF GPU process's DMA-BUF export
    // and our wgpu Vulkan import to land on the same physical GPU. On a
    // hybrid-graphics laptop, Chromium's GPU process defaults to the
    // display-driving iGPU while wgpu's HighPerformance pick can land on
    // the discrete GPU; Chromium also strips most env vars from its
    // child processes, so there's no reliable way to force both onto
    // the same device from here. CPU OSR (on_paint, plain memcpy) has no
    // such cross-device requirement, so that's what's wired up for now.
    let window_info = cef::WindowInfo {
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

    let browser_settings = cef::BrowserSettings {
        windowless_frame_rate: 60,
        ..Default::default()
    };
    let mut context = cef::request_context_create_context(
        Some(&cef::RequestContextSettings::default()),
        Some(&mut RequestContextHandlerBuilder::build(
            OsrRequestContextHandler {},
        )),
    );

    let browser = cef::browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut ClientBuilder::build(render_handler)),
        Some(&home_page.into()),
        Some(&browser_settings),
        None,
        context.as_mut(),
    )
    .expect("failed to create CEF browser");

    BrowserState {
        browser,
        size: browser_size,
    }
}
