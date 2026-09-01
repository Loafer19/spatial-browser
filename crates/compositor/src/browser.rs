// One canvas page: own CEF browser, texture, quad, world rect.
// request_context None → shared persistent global (cookies/logins across pages).

use crate::output::{GpuState, PageQuad, Rect};
use cef_bridge::{ClientBuilder, OsrRenderHandler};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use winit::window::Window;

// Under wgpu's default 8192 max_texture_dimension_2d.
const MAX_PAGE_DIMENSION: f32 = 4096.0;

// Settings fps (60/90/120); read by spawn + pushed live to open hosts.
static TARGET_FRAME_RATE: AtomicU32 = AtomicU32::new(60);

/// Startup + every Settings frame-rate change.
pub fn set_target_frame_rate(fps: u32) {
    TARGET_FRAME_RATE.store(fps, Ordering::Relaxed);
}

pub struct Page {
    pub browser: cef::Browser,
    pub rect: Rect,
    pub quad: PageQuad,
    // CEF DIP size; synced from `rect` / scale_factor on move/resize.
    size: Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
    texture: Rc<RefCell<Option<wgpu::BindGroup>>>,
    // Pre-Ctrl+Space rect while zoomed; persistence must save this, not `rect`.
    pub zoomed_from: Option<Rect>,
    // Utility pages — skipped by session/workspace save (stale snapshot otherwise).
    pub ephemeral: bool,
    // Runtime-only (not persisted). Cleared on real navigation; toggle-off reloads.
    pub reader_mode: std::cell::Cell<bool>,
    /// Top-edge load bar progress (0..1). Runtime-only; not persisted.
    pub load_progress: std::cell::Cell<f32>,
    /// True while CEF reports loading. Runtime-only; not persisted.
    pub load_active: std::cell::Cell<bool>,
}

impl Page {
    pub fn texture(&self) -> Option<wgpu::BindGroup> {
        self.texture.borrow().clone()
    }

    /// Current URL from CEF (includes in-page navigations).
    pub fn url(&self) -> String {
        cef::ImplBrowser::main_frame(&self.browser)
            .map(|frame| cef::CefString::from(&cef::ImplFrame::url(&frame)).to_string())
            .unwrap_or_default()
    }

    /// World rect to persist (pre-Ctrl+Space size while zoomed-to-canvas).
    pub fn layout_rect(&self) -> Rect {
        self.zoomed_from.unwrap_or(self.rect)
    }

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
    // Shared-texture needs CEF GPU + wgpu on the same device (see output::).
    let shared_texture = crate::output::osr_shared_texture_enabled();
    let window_info = cef::WindowInfo {
        windowless_rendering_enabled: true as _,
        shared_texture_enabled: shared_texture as _,
        external_begin_frame_enabled: true as _,
        ..Default::default()
    };
    if shared_texture {
        log::info!("CEF OSR: shared GPU texture enabled");
    } else {
        log::info!("CEF OSR: CPU paint path");
    }

    let device_scale_factor = window.scale_factor();
    let (render_handler, handles) = OsrRenderHandler::new(
        gpu.device.clone(),
        gpu.queue.clone(),
        gpu.texture_bind_group_layout.clone(),
        device_scale_factor as _,
        winit::dpi::PhysicalSize::new(rect.w, rect.h).to_logical::<f32>(device_scale_factor),
    );

    let browser_settings = cef::BrowserSettings {
        windowless_frame_rate: TARGET_FRAME_RATE.load(Ordering::Relaxed) as _,
        ..Default::default()
    };
    let browser = cef::browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut ClientBuilder::build(render_handler)),
        Some(&url.into()),
        Some(&browser_settings),
        None,
        None, // shared persistent global request context
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
        reader_mode: std::cell::Cell::new(false),
        load_progress: std::cell::Cell::new(0.0),
        // Utility pages (help/settings/…) skip the load bar.
        load_active: std::cell::Cell::new(!ephemeral),
    }
}
