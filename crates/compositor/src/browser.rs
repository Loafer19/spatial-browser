// Spawning a CEF browser instance for one page of the spatial canvas:
// its own render handler (own texture + own logical size, independent of
// every other page), its own GPU quad, and its own canvas-space rect.

use crate::output::{GpuState, PageQuad, Rect};
use cef_bridge::{
    ClientBuilder, OsrRenderHandler, OsrRequestContextHandler, RequestContextHandlerBuilder,
};
use std::cell::RefCell;
use std::rc::Rc;
use winit::window::Window;

// Comfortably under wgpu's default 8192 max_texture_dimension_2d (see
// Rect::clamp_size) — no single floating page plausibly needs more.
const MAX_PAGE_DIMENSION: f32 = 4096.0;

pub struct Page {
    pub browser: cef::Browser,
    pub rect: Rect,
    pub quad: PageQuad,
    // CEF's own logical (DIP) view size, kept in sync with `rect` (scaled
    // by the window's device_scale_factor) whenever the page is moved or
    // resized on the canvas.
    size: Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
    texture: Rc<RefCell<Option<wgpu::BindGroup>>>,
    // The rect this page had before the zoom-toggle hotkey enlarged it —
    // `Some` while zoomed in, restored and cleared on toggling back out.
    pub zoomed_from: Option<Rect>,
    // True for generated utility pages (F1 help, the bookmarks list) —
    // persistence.rs skips these when saving. Without this, a bookmarks-
    // list page opened once gets frozen into session.json and reopens on
    // every future launch as a stale snapshot: outdated content, and
    // bookmark:// indices inside it that no longer match the current
    // bookmarks list.
    pub ephemeral: bool,
}

impl Page {
    pub fn texture(&self) -> Option<wgpu::BindGroup> {
        // wgpu::BindGroup is cheap to clone (it's a ref-counted handle),
        // so callers can hold this independent of the page's own borrow.
        self.texture.borrow().clone()
    }

    /// Reads this page's *current* URL straight from CEF rather than
    /// tracking it ourselves, so in-page navigation (a clicked link,
    /// back/forward) is reflected too, not just the URL it was spawned
    /// with.
    pub fn url(&self) -> String {
        cef::ImplBrowser::main_frame(&self.browser)
            .map(|frame| cef::CefString::from(&cef::ImplFrame::url(&frame)).to_string())
            .unwrap_or_default()
    }

    /// Update this page's canvas rect and let CEF know its view resized.
    pub fn set_rect(&mut self, rect: Rect, scale_factor: f64) {
        let rect = rect.clamp_size(MAX_PAGE_DIMENSION);
        self.rect = rect;
        *self.size.borrow_mut() =
            winit::dpi::PhysicalSize::new(rect.w, rect.h).to_logical::<f32>(scale_factor);
        if let Some(host) = cef::ImplBrowser::host(&self.browser) {
            cef::ImplBrowserHost::was_resized(&host);
        }
    }
}

pub fn spawn(gpu: &GpuState, window: &Window, url: &str, rect: Rect, ephemeral: bool) -> Page {
    let rect = rect.clamp_size(MAX_PAGE_DIMENSION);
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
    let (render_handler, handles) = OsrRenderHandler::new(
        gpu.device.clone(),
        gpu.queue.clone(),
        gpu.texture_bind_group_layout.clone(),
        device_scale_factor as _,
        winit::dpi::PhysicalSize::new(rect.w, rect.h).to_logical::<f32>(device_scale_factor),
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
        Some(&url.into()),
        Some(&browser_settings),
        None,
        context.as_mut(),
    )
    .expect("failed to create CEF browser");

    Page {
        browser,
        rect,
        quad: PageQuad::new(&gpu.device, &gpu.style_bind_group_layout),
        size: handles.size,
        texture: handles.texture,
        zoomed_from: None,
        ephemeral,
    }
}
