//! FFI bridge to CEF (Chromium Embedded Framework), off-screen rendering
//! with shared GPU textures. Each `OsrRenderHandler` owns its own texture
//! slot (returned from `OsrRenderHandler::new`), so multiple simultaneous
//! browser instances — multiple pages on the spatial canvas — each paint
//! into their own texture rather than a shared global.

use cef::{
    self, BrowserProcessHandler, ImplBrowserProcessHandler, WrapBrowserProcessHandler, rc::Rc, *,
};
use cef::{ImplRequestContextHandler, RequestContextHandler, WrapRequestContextHandler};
use cef::{ImplDisplayHandler, WrapDisplayHandler};
use cef::{ImplRequest, ImplRequestHandler, WrapRequestHandler};
use std::cell::RefCell;
use winit::window::CursorIcon;

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

#[derive(Clone)]
pub struct OsrRenderHandler {
    device_scale_factor: f32,
    size: std::rc::Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    // Bind group layout owned by the compositor's render pipeline. wgpu
    // matches bind groups to a pipeline by BindGroupLayout identity, not
    // structure, so the bind group built on each paint must be built
    // against this exact layout, not a freshly created (if structurally
    // identical) one.
    texture_bind_group_layout: wgpu::BindGroupLayout,
    // This page's own texture slot, read once per frame by the
    // compositor's render loop for this page's quad specifically.
    texture: std::rc::Rc<RefCell<Option<wgpu::BindGroup>>>,
}

/// Handles returned alongside an `OsrRenderHandler` for the compositor to
/// read/update from outside CEF's callbacks: `size` is written by the
/// compositor (page resize) and read by `view_rect`; `texture` is written
/// by `on_accelerated_paint`/`on_paint` and read by the compositor's
/// render loop.
pub struct OsrRenderHandles {
    pub size: std::rc::Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
    pub texture: std::rc::Rc<RefCell<Option<wgpu::BindGroup>>>,
}

impl OsrRenderHandler {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        texture_bind_group_layout: wgpu::BindGroupLayout,
        device_scale_factor: f32,
        size: winit::dpi::LogicalSize<f32>,
    ) -> (Self, OsrRenderHandles) {
        let size = std::rc::Rc::new(RefCell::new(size));
        let texture = std::rc::Rc::new(RefCell::new(None));
        (
            Self {
                size: size.clone(),
                device_scale_factor,
                device,
                queue,
                texture_bind_group_layout,
                texture: texture.clone(),
            },
            OsrRenderHandles { size, texture },
        )
    }
}

wrap_render_handler! {
    pub struct RenderHandlerBuilder {
        handler: OsrRenderHandler,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let size = self.handler.size.borrow();
                // size must be non-zero
                if size.width > 0.0 && size.height > 0.0 {
                    rect.width = size.width as _;
                    rect.height = size.height as _;
                }
            }
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> ::std::os::raw::c_int {
            if let Some(screen_info) = screen_info {
                screen_info.device_scale_factor = self.handler.device_scale_factor;
                return true as _;
            }
            false as _
        }

        fn screen_point(
            &self,
            _browser: Option<&mut Browser>,
            _view_x: ::std::os::raw::c_int,
            _view_y: ::std::os::raw::c_int,
            _screen_x: Option<&mut ::std::os::raw::c_int>,
            _screen_y: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            false as _
        }

        #[cfg(feature = "accelerated_osr")]
        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            let Some(info) = info else { return };

            let src_texture = {
                use cef::osr_texture_import::shared_texture_handle::SharedTextureHandle;

                if type_ != PaintElementType::default() {
                    return;
                }

                let shared_handle = SharedTextureHandle::new(info);
                if let SharedTextureHandle::Unsupported = shared_handle {
                    log::warn!("platform does not support accelerated OSR painting");
                    return;
                }

                match shared_handle.import_texture(&self.handler.device) {
                    Ok(texture) => texture,
                    Err(e) => {
                        log::warn!("failed to import CEF shared texture: {e:?}");
                        return;
                    }
                }
            };

            let sampler = self.handler.device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            });

            let bind_group = self.handler.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Cef Texture Bind Group"),
                layout: &self.handler.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src_texture.create_view(
                            &wgpu::TextureViewDescriptor {
                                label: Some("Cef Texture View"),
                                ..Default::default()
                            },
                        )),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            self.handler.texture.borrow_mut().replace(bind_group);
        }

        // CPU/software OSR path: CEF hands over a raw BGRA pixel buffer
        // instead of a GPU texture handle. Slower and costs a memcpy per
        // frame, but doesn't depend on the Vulkan external-memory/DMA-BUF
        // import that `on_accelerated_paint` needs — which requires a
        // dedicated device memory allocation that can fail under GPU
        // memory pressure (small VRAM budgets, many concurrent GPU
        // clients) even though there's nothing wrong with the page or the
        // pipeline. This is the fallback CEF calls when
        // `shared_texture_enabled` is off in `WindowInfo`.
        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            if type_ != PaintElementType::default() || buffer.is_null() || width <= 0 || height <= 0
            {
                return;
            }

            let (width, height) = (width as u32, height as u32);
            let byte_len = (width * height * 4) as usize;
            let pixels = unsafe { std::slice::from_raw_parts(buffer, byte_len) };

            let texture = self.handler.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Cef CPU Paint Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            self.handler.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * width),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );

            let sampler = self.handler.device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            });

            let bind_group = self.handler.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Cef CPU Texture Bind Group"),
                layout: &self.handler.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture.create_view(
                            &wgpu::TextureViewDescriptor {
                                label: Some("Cef CPU Texture View"),
                                ..Default::default()
                            },
                        )),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            self.handler.texture.borrow_mut().replace(bind_group);
        }
    }
}

impl RenderHandlerBuilder {
    pub fn build(handler: OsrRenderHandler) -> RenderHandler {
        Self::new(handler)
    }
}

thread_local! {
    // Set by `OsrDisplayHandler::on_cursor_change`, read once per frame by
    // the compositor's redraw handler. CEF doesn't drive the OS cursor
    // itself in windowless/OSR mode (it has no native window to do it
    // through), so the embedder has to apply the shape the page wants.
    // Global (not per-page) is fine: only the page currently under the
    // mouse gets cursor-change events, so this naturally tracks whichever
    // page's cursor should be showing.
    pub static CURSOR: RefCell<Option<CursorIcon>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub struct OsrDisplayHandler {}

wrap_display_handler! {
    pub struct DisplayHandlerBuilder {
        handler: OsrDisplayHandler,
    }

    impl DisplayHandler {
        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            _cursor: ::std::os::raw::c_ulong,
            type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> ::std::os::raw::c_int {
            CURSOR.with_borrow_mut(|cursor| {
                cursor.replace(cef_cursor_to_winit(type_));
            });
            true as _
        }
    }
}

impl DisplayHandlerBuilder {
    pub fn build(handler: OsrDisplayHandler) -> DisplayHandler {
        Self::new(handler)
    }
}

// CEF's CT_* cursor types (include/internal/cef_types.h) mapped to winit's
// platform-independent CursorIcon. Some CEF distinctions winit doesn't
// have a dedicated icon for (panning directions, DND variants) fall back
// to their closest equivalent rather than a 1:1 match.
fn cef_cursor_to_winit(type_: CursorType) -> CursorIcon {
    match type_ {
        CursorType::POINTER => CursorIcon::Default,
        CursorType::CROSS => CursorIcon::Crosshair,
        CursorType::HAND => CursorIcon::Pointer,
        CursorType::IBEAM => CursorIcon::Text,
        CursorType::WAIT => CursorIcon::Wait,
        CursorType::HELP => CursorIcon::Help,
        CursorType::EASTRESIZE => CursorIcon::EResize,
        CursorType::NORTHRESIZE => CursorIcon::NResize,
        CursorType::NORTHEASTRESIZE => CursorIcon::NeResize,
        CursorType::NORTHWESTRESIZE => CursorIcon::NwResize,
        CursorType::SOUTHRESIZE => CursorIcon::SResize,
        CursorType::SOUTHEASTRESIZE => CursorIcon::SeResize,
        CursorType::SOUTHWESTRESIZE => CursorIcon::SwResize,
        CursorType::WESTRESIZE => CursorIcon::WResize,
        CursorType::NORTHSOUTHRESIZE => CursorIcon::NsResize,
        CursorType::EASTWESTRESIZE => CursorIcon::EwResize,
        CursorType::NORTHEASTSOUTHWESTRESIZE => CursorIcon::NeswResize,
        CursorType::NORTHWESTSOUTHEASTRESIZE => CursorIcon::NwseResize,
        CursorType::COLUMNRESIZE => CursorIcon::ColResize,
        CursorType::ROWRESIZE => CursorIcon::RowResize,
        CursorType::MIDDLEPANNING
        | CursorType::EASTPANNING
        | CursorType::NORTHPANNING
        | CursorType::NORTHEASTPANNING
        | CursorType::NORTHWESTPANNING
        | CursorType::SOUTHPANNING
        | CursorType::SOUTHEASTPANNING
        | CursorType::SOUTHWESTPANNING
        | CursorType::WESTPANNING
        | CursorType::MIDDLE_PANNING_VERTICAL
        | CursorType::MIDDLE_PANNING_HORIZONTAL => CursorIcon::AllScroll,
        CursorType::MOVE => CursorIcon::Move,
        CursorType::VERTICALTEXT => CursorIcon::VerticalText,
        CursorType::CELL => CursorIcon::Cell,
        CursorType::CONTEXTMENU => CursorIcon::ContextMenu,
        CursorType::ALIAS => CursorIcon::Alias,
        CursorType::PROGRESS => CursorIcon::Progress,
        CursorType::NODROP => CursorIcon::NoDrop,
        CursorType::COPY | CursorType::DND_COPY => CursorIcon::Copy,
        CursorType::NONE => CursorIcon::Default,
        CursorType::NOTALLOWED => CursorIcon::NotAllowed,
        CursorType::ZOOMIN => CursorIcon::ZoomIn,
        CursorType::ZOOMOUT => CursorIcon::ZoomOut,
        CursorType::GRAB | CursorType::DND_MOVE => CursorIcon::Grab,
        CursorType::GRABBING => CursorIcon::Grabbing,
        CursorType::DND_NONE => CursorIcon::NoDrop,
        CursorType::DND_LINK => CursorIcon::Alias,
        _ => CursorIcon::Default,
    }
}

/// What the bookmarks-list page (compositor::hotkeys) asked for, parsed
/// from a `bookmark://...` link/form it navigated to.
pub enum BookmarkAction {
    Open(usize),
    Delete(usize),
    /// index, new title, new folder (empty string = clear/ungrouped)
    Rename(usize, String, String),
}

thread_local! {
    // Set by `OsrRequestHandler::on_before_browse` when a page navigates
    // to a `bookmark://...` link or form (only the generated
    // bookmarks-list page ever produces one — see compositor::hotkeys —
    // so a global slot is safe: no other page's real navigation can
    // collide with it). The browser identifier (CEF's own per-instance
    // id) tags *which* page asked, so the compositor can reload that
    // exact bookmarks-list page in place after a delete/rename rather
    // than guessing. Read once per frame by the compositor's redraw
    // handler.
    pub static PENDING_BOOKMARK: RefCell<Option<(i32, BookmarkAction)>> = const { RefCell::new(None) };
}

/// Minimal `application/x-www-form-urlencoded` decode (`+` -> space,
/// `%XX` -> byte) — just enough for the rename form's `title`/`folder`
/// values, without pulling in a URL crate for it.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Pulls a query parameter's decoded value out of `a=1&b=2`-style text.
fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| percent_decode(value))
    })
}

fn parse_bookmark_action(url: &str) -> Option<BookmarkAction> {
    let rest = url.strip_prefix("bookmark://")?;
    if let Some(index) = rest.strip_prefix("open/") {
        return index.parse().ok().map(BookmarkAction::Open);
    }
    if let Some(index) = rest.strip_prefix("delete/") {
        return index.parse().ok().map(BookmarkAction::Delete);
    }
    if let Some(after) = rest.strip_prefix("rename/") {
        let (index, query) = after.split_once('?').unwrap_or((after, ""));
        let index = index.parse().ok()?;
        let title = query_param(query, "title").unwrap_or_default();
        let folder = query_param(query, "folder").unwrap_or_default();
        return Some(BookmarkAction::Rename(index, title, folder));
    }
    None
}

/// What the omnibox page (compositor::hotkeys::omnibox_page_url) asked
/// for, parsed from an `omnibox://go?q=...&url=...` navigation it made.
/// `raw` is exactly what the user typed (for history); `url` is what the
/// page's own JS already resolved it to (URL detection / @prefix search
/// engines / default search) — the compositor doesn't need to know that
/// resolution logic, just where to navigate.
pub struct OmniboxSubmit {
    pub raw: String,
    pub url: String,
}

thread_local! {
    // Same shape/reasoning as PENDING_BOOKMARK, for the omnibox page.
    pub static PENDING_OMNIBOX: RefCell<Option<(i32, OmniboxSubmit)>> = const { RefCell::new(None) };
}

fn parse_omnibox_submit(url: &str) -> Option<OmniboxSubmit> {
    let query = url.strip_prefix("omnibox://go?")?;
    Some(OmniboxSubmit {
        raw: query_param(query, "q")?,
        url: query_param(query, "url")?,
    })
}

#[derive(Clone)]
pub struct OsrRequestHandler {}

wrap_request_handler! {
    pub struct RequestHandlerBuilder {
        handler: OsrRequestHandler,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let (Some(browser), Some(request)) = (browser, request) else {
                return false as _;
            };
            let url = cef::CefString::from(&request.url()).to_string();
            let id = browser.identifier();
            if let Some(action) = parse_bookmark_action(&url) {
                PENDING_BOOKMARK.with_borrow_mut(|pending| *pending = Some((id, action)));
                return true as _; // cancel navigation — the compositor acts on it instead
            }
            if let Some(submit) = parse_omnibox_submit(&url) {
                PENDING_OMNIBOX.with_borrow_mut(|pending| *pending = Some((id, submit)));
                return true as _;
            }
            false as _
        }
    }
}

impl RequestHandlerBuilder {
    pub fn build(handler: OsrRequestHandler) -> cef::RequestHandler {
        Self::new(handler)
    }
}

wrap_client! {
    pub struct ClientBuilder {
        render_handler: RenderHandler,
        display_handler: DisplayHandler,
        request_handler: cef::RequestHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<cef::RenderHandler> {
            Some(self.render_handler.clone())
        }

        fn display_handler(&self) -> Option<cef::DisplayHandler> {
            Some(self.display_handler.clone())
        }

        fn request_handler(&self) -> Option<cef::RequestHandler> {
            Some(self.request_handler.clone())
        }
    }
}

impl ClientBuilder {
    pub fn build(render_handler: OsrRenderHandler) -> Client {
        Self::new(
            RenderHandlerBuilder::build(render_handler),
            DisplayHandlerBuilder::build(OsrDisplayHandler {}),
            RequestHandlerBuilder::build(OsrRequestHandler {}),
        )
    }
}

#[derive(Clone)]
pub struct OsrRequestContextHandler {}

wrap_request_context_handler! {
    pub struct RequestContextHandlerBuilder {
        handler: OsrRequestContextHandler,
    }

    impl RequestContextHandler {}
}

impl RequestContextHandlerBuilder {
    pub fn build(handler: OsrRequestContextHandler) -> RequestContextHandler {
        Self::new(handler)
    }
}
