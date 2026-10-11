// One canvas page: own CEF browser, texture, quad, world rect.
// request_context None → shared persistent global (cookies/logins across pages).

use crate::output::{GpuState, PageQuad, Rect};
use cef_bridge::{ClientBuilder, OsrRenderHandler, OsrRenderHandles};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
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

    /// Changes on every CEF paint: the bind group is replaced per paint, and
    /// cef_bridge::note_paint covers paint paths that reuse it (frame_pacer).
    pub fn paint_token(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.texture.borrow().hash(&mut hasher);
        cef_bridge::paint_count(cef::ImplBrowser::identifier(&self.browser)).hash(&mut hasher);
        hasher.finish()
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

/// What CEF needs to create one windowless page browser, plus the handles
/// the compositor reads its texture/size through. Shared by `spawn` and
/// popups (which CEF creates itself; see `install_popup_factory`).
struct OsrSetup {
    window_info: cef::WindowInfo,
    client: cef::Client,
    settings: cef::BrowserSettings,
    handles: OsrRenderHandles,
}

fn osr_setup(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    scale_factor: f64,
    rect: Rect,
) -> OsrSetup {
    // Shared-texture needs CEF GPU + wgpu on the same device (see output::).
    let shared_texture = crate::output::osr_shared_texture_enabled();
    let window_info = cef::WindowInfo {
        windowless_rendering_enabled: true as _,
        shared_texture_enabled: shared_texture as _,
        external_begin_frame_enabled: true as _,
        ..Default::default()
    };
    // Per browser: debug only (startup options are logged once at info).
    log::debug!(
        "CEF OSR: {}",
        if shared_texture {
            "shared GPU texture"
        } else {
            "CPU paint path"
        }
    );
    let (render_handler, handles) = OsrRenderHandler::new(
        device.clone(),
        queue.clone(),
        layout.clone(),
        scale_factor as _,
        winit::dpi::PhysicalSize::new(rect.w, rect.h).to_logical::<f32>(scale_factor),
    );
    OsrSetup {
        window_info,
        client: ClientBuilder::build(render_handler),
        settings: cef::BrowserSettings {
            windowless_frame_rate: TARGET_FRAME_RATE.load(Ordering::Relaxed) as _,
            ..Default::default()
        },
        handles,
    }
}

impl Page {
    fn new(
        gpu: &GpuState,
        browser: cef::Browser,
        rect: Rect,
        handles: OsrRenderHandles,
        ephemeral: bool,
    ) -> Self {
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

    /// A popup CEF created from our factory: wrap it as a normal page at
    /// `rect` (its render handler started at a default size).
    pub fn adopt_popup(
        gpu: &GpuState,
        browser: cef::Browser,
        handles: OsrRenderHandles,
        rect: Rect,
    ) -> Self {
        let mut page = Page::new(
            gpu,
            browser,
            rect.clamp_size(MAX_PAGE_DIMENSION),
            handles,
            false,
        );
        page.set_rect(page.rect, gpu.window.scale_factor());
        page
    }
}

/// Size a popup browser starts at, before `adopt_popup` places it.
const POPUP_START_SIZE: (f32, f32) = (1024.0, 768.0);

/// Let CEF create allowed popups as real windowless browsers (child of the
/// opener: `window.opener` / `postMessage` work, e.g. OAuth) instead of the
/// compositor cancelling and re-spawning them, which left the renderer
/// waiting on a browser that never came ("Timeout of new browser info").
pub fn install_popup_factory(gpu: &GpuState) {
    let device = gpu.device.clone();
    let queue = gpu.queue.clone();
    let layout = gpu.texture_bind_group_layout.clone();
    let window = gpu.window.clone();
    cef_bridge::set_popup_factory(Box::new(move || {
        let (w, h) = POPUP_START_SIZE;
        let setup = osr_setup(
            &device,
            &queue,
            &layout,
            window.scale_factor(),
            Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
        );
        cef_bridge::PopupSetup {
            window_info: setup.window_info,
            client: setup.client,
            settings: setup.settings,
            handles: setup.handles,
        }
    }));
}

/// One pre-started, hidden browser: CEF browser + renderer process already
/// up, so opening a tile is a navigation instead of a process launch.
struct Warm {
    browser: cef::Browser,
    handles: OsrRenderHandles,
}

/// Which pool slot a page can come from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WarmKind {
    /// Idles on a blank spatial:// page: its process is already the
    /// built-in pages' site, so help/lists/omnibox open without a swap.
    Builtin,
    /// Idles on about:blank (unlocked process) for the next web tile.
    Web,
}

#[derive(Default)]
struct Pool {
    builtin: Option<Warm>,
    web: Option<Warm>,
    /// Refill waits for this: startup settled, or the last take loaded.
    quiet_since: Option<Instant>,
}

thread_local! {
    static POOL: RefCell<Pool> = RefCell::new(Pool::default());
}

/// Let startup and a just-opened tile load before warming a replacement.
const REFILL_DELAY: Duration = Duration::from_secs(2);

/// Pool slot a spawn of `url` can use, if any.
fn warm_kind(url: &str, ephemeral: bool) -> Option<WarmKind> {
    if ephemeral && cef_bridge::spatial::parse_page_url(url).is_some() {
        Some(WarmKind::Builtin)
    } else if !ephemeral && crate::external::is_web_url(url) {
        Some(WarmKind::Web)
    } else {
        None
    }
}

fn take_warm(kind: WarmKind) -> Option<Warm> {
    POOL.with_borrow_mut(|pool| {
        pool.quiet_since = Some(Instant::now());
        match kind {
            WarmKind::Builtin => pool.builtin.take(),
            WarmKind::Web => pool.web.take(),
        }
    })
}

fn warm_up(gpu: &GpuState, kind: WarmKind) -> Option<Warm> {
    let (w, h) = POPUP_START_SIZE;
    let OsrSetup {
        window_info,
        mut client,
        settings,
        handles,
    } = osr_setup(
        &gpu.device,
        &gpu.queue,
        &gpu.texture_bind_group_layout,
        gpu.window.scale_factor(),
        Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        },
    );
    let url = match kind {
        WarmKind::Builtin => crate::pages::serve(crate::pages::kind::BLANK, String::new()),
        WarmKind::Web => "about:blank".to_string(),
    };
    let browser = cef::browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut client),
        Some(&url.as_str().into()),
        Some(&settings),
        None,
        None,
    )?;
    if kind == WarmKind::Builtin {
        cef_bridge::spatial::bind(&url, cef::ImplBrowser::identifier(&browser));
    }
    // Hidden: no paints, throttled timers until it becomes a tile.
    if let Some(host) = cef::ImplBrowser::host(&browser) {
        cef::ImplBrowserHost::was_hidden(&host, true as _);
    }
    log::debug!("warm {kind:?} browser ready");
    Some(Warm { browser, handles })
}

/// Called every tick: refill an empty pool slot once things are quiet.
pub fn refill_pool(gpu: &GpuState, now: Instant) {
    let due = POOL.with_borrow_mut(|pool| {
        let since = *pool.quiet_since.get_or_insert(now);
        if now.saturating_duration_since(since) < REFILL_DELAY {
            return None;
        }
        if pool.builtin.is_none() {
            Some(WarmKind::Builtin)
        } else if pool.web.is_none() {
            Some(WarmKind::Web)
        } else {
            None
        }
    });
    let Some(kind) = due else { return };
    let warm = warm_up(gpu, kind);
    POOL.with_borrow_mut(|pool| {
        // One per tick; the next waits its own delay so loads don't pile up.
        pool.quiet_since = Some(now);
        match kind {
            WarmKind::Builtin => pool.builtin = warm,
            WarmKind::Web => pool.web = warm,
        }
    });
}

/// Close and drop the pooled browsers (before cef::shutdown).
pub fn close_pool() {
    let warm: Vec<Warm> = POOL.with_borrow_mut(|pool| {
        [pool.builtin.take(), pool.web.take()]
            .into_iter()
            .flatten()
            .collect()
    });
    for w in warm {
        if let Some(host) = cef::ImplBrowser::host(&w.browser) {
            cef::ImplBrowserHost::close_browser(&host, true as _);
        }
    }
}

/// Turn a pooled browser into a tile showing `url`.
fn adopt_warm(gpu: &GpuState, warm: Warm, url: &str, rect: Rect, ephemeral: bool) -> Page {
    let id = cef::ImplBrowser::identifier(&warm.browser);
    if ephemeral && !cef_bridge::spatial::bind(url, id) {
        log::warn!("built-in page token not bound to warm browser {id}");
    }
    let mut page = Page::new(gpu, warm.browser, rect, warm.handles, ephemeral);
    page.set_rect(rect, gpu.window.scale_factor());
    if let Some(host) = cef::ImplBrowser::host(&page.browser) {
        cef::ImplBrowserHost::was_hidden(&host, false as _);
    }
    if let Some(frame) = cef::ImplBrowser::main_frame(&page.browser) {
        cef::ImplFrame::load_url(&frame, Some(&url.into()));
    }
    page
}

pub fn spawn(gpu: &GpuState, window: &Window, url: &str, rect: Rect, ephemeral: bool) -> Page {
    let rect = rect.clamp_size(MAX_PAGE_DIMENSION);
    if let Some(warm) = warm_kind(url, ephemeral).and_then(take_warm) {
        return adopt_warm(gpu, warm, url, rect, ephemeral);
    }
    let OsrSetup {
        window_info,
        mut client,
        settings,
        handles,
    } = osr_setup(
        &gpu.device,
        &gpu.queue,
        &gpu.texture_bind_group_layout,
        window.scale_factor(),
        rect,
    );
    let browser = cef::browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut client),
        Some(&url.into()),
        Some(&settings),
        None,
        None, // shared persistent global request context
    )
    .expect("failed to create CEF browser");
    // Built-in page: from now on only this browser is served its token, and
    // only its actions count as internal (pending_actions::action_source).
    if ephemeral && cef_bridge::spatial::parse_page_url(url).is_some() {
        let id = cef::ImplBrowser::identifier(&browser);
        if !cef_bridge::spatial::bind(url, id) {
            log::warn!("built-in page token not bound to browser {id}");
        }
    }
    Page::new(gpu, browser, rect, handles, ephemeral)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_slot_by_url_and_tile_kind() {
        let builtin = crate::pages::serve(crate::pages::kind::HELP, String::new());
        assert_eq!(warm_kind(&builtin, true), Some(WarmKind::Builtin));
        assert_eq!(warm_kind("https://a.test/", false), Some(WarmKind::Web));
        // A built-in URL in a non-ephemeral tile, or a web URL in an
        // ephemeral one, never takes a pooled browser.
        assert_eq!(warm_kind(&builtin, false), None);
        assert_eq!(warm_kind("https://a.test/", true), None);
        assert_eq!(warm_kind("file:///etc/passwd", false), None);
    }
}
