// The paint path: CEF hands over either a shared GPU texture handle
// (`on_accelerated_paint`, feature-gated — see main README's Status for
// why it's not the default yet) or a raw CPU pixel buffer
// (`on_paint`), and either way this turns it into a wgpu texture/bind
// group the compositor's render pipeline can draw. Each spawned page
// gets its own `OsrRenderHandler`/texture slot (browser.rs), not a
// shared one, so pages never race each other for the same buffer.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

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
