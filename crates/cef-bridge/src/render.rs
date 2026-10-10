// CEF paint → wgpu bind group. Per-page texture slot (GPU shared or CPU path).

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

#[derive(Clone)]
pub struct OsrRenderHandler {
    device_scale_factor: f32,
    size: std::rc::Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    // Must be the compositor pipeline's layout identity, not a lookalike.
    texture_bind_group_layout: wgpu::BindGroupLayout,
    texture: std::rc::Rc<RefCell<Option<wgpu::BindGroup>>>,
    // Created once per page, shared by both paint paths.
    sampler: wgpu::Sampler,
    // CPU path: the page's texture, reused while its size holds.
    cpu: std::rc::Rc<RefCell<Option<CpuTarget>>>,
}

struct CpuTarget {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

#[derive(Clone, Copy)]
enum PaintPath {
    SharedTexture,
    Cpu,
}

/// Logs (once per path per process) which OSR paint path actually delivers
/// frames: CEF falls back to CPU on_paint on its own when it can't share
/// textures, so the configured mode alone doesn't say.
fn log_paint_path_once(path: PaintPath) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static SHARED: AtomicBool = AtomicBool::new(false);
    static CPU: AtomicBool = AtomicBool::new(false);
    let (seen, name) = match path {
        PaintPath::SharedTexture => (&SHARED, "shared GPU texture (dma-buf import)"),
        PaintPath::Cpu => (&CPU, "CPU on_paint upload"),
    };
    if !seen.swap(true, Ordering::Relaxed) {
        log::info!("OSR frames arriving via {name}");
    }
}

/// CEF dirty rects as `(x, y, w, h)` clipped to a `width`×`height` view;
/// empty input means the whole view.
pub fn clip_dirty(
    rects: &[(i32, i32, i32, i32)],
    width: u32,
    height: u32,
) -> Vec<(u32, u32, u32, u32)> {
    if rects.is_empty() {
        return vec![(0, 0, width, height)];
    }
    rects
        .iter()
        .filter_map(|&(x, y, w, h)| {
            let x0 = x.clamp(0, width as i32) as u32;
            let y0 = y.clamp(0, height as i32) as u32;
            let x1 = x.saturating_add(w).clamp(0, width as i32) as u32;
            let y1 = y.saturating_add(h).clamp(0, height as i32) as u32;
            (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
        })
        .collect()
}

/// Shared size/texture handles for the compositor outside CEF callbacks.
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        (
            Self {
                size: size.clone(),
                device_scale_factor,
                device,
                queue,
                texture_bind_group_layout,
                texture: texture.clone(),
                sampler,
                cpu: std::rc::Rc::new(RefCell::new(None)),
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
            browser: Option<&mut Browser>,
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
                        resource: wgpu::BindingResource::Sampler(&self.handler.sampler),
                    },
                ],
            });

            // A later CPU paint must rebuild its bind group, not reuse a stale one.
            self.handler.cpu.borrow_mut().take();
            self.handler.texture.borrow_mut().replace(bind_group);
            if let Some(browser) = browser {
                crate::paint_signal::note_paint(browser.identifier());
            }
            log_paint_path_once(PaintPath::SharedTexture);
        }

        // CPU OSR fallback when shared_texture_enabled is off.
        // Signature is fixed by the cef-rs trait, so it can't be `unsafe fn`.
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        fn on_paint(
            &self,
            browser: Option<&mut Browser>,
            type_: PaintElementType,
            dirty_rects: Option<&[Rect]>,
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
            // CEF guarantees `buffer` holds width*height BGRA pixels for this call;
            // dirty rects are in the same pixel space.
            let pixels = unsafe { std::slice::from_raw_parts(buffer, byte_len) };

            let mut cpu = self.handler.cpu.borrow_mut();
            let reuse = cpu.as_ref().is_some_and(|t| t.width == width && t.height == height);
            if !reuse {
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
                            resource: wgpu::BindingResource::Sampler(&self.handler.sampler),
                        },
                    ],
                });
                *cpu = Some(CpuTarget {
                    width,
                    height,
                    texture,
                    bind_group,
                });
            }
            let Some(target) = cpu.as_ref() else { return };

            // A fresh texture needs the whole frame; otherwise only what changed.
            let rects: Vec<(i32, i32, i32, i32)> = match dirty_rects {
                Some(rects) if reuse => rects.iter().map(|r| (r.x, r.y, r.width, r.height)).collect(),
                _ => Vec::new(),
            };
            for (x, y, w, h) in clip_dirty(&rects, width, height) {
                self.handler.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &target.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: (u64::from(y) * u64::from(width) + u64::from(x)) * 4,
                        bytes_per_row: Some(4 * width),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
            }

            if !reuse {
                self.handler.texture.borrow_mut().replace(target.bind_group.clone());
            }
            // The bind group is reused, so the frame pacer can't see this
            // paint by identity; tell it.
            if let Some(browser) = browser {
                crate::paint_signal::note_paint(browser.identifier());
            }
            log_paint_path_once(PaintPath::Cpu);
        }
    }
}

impl RenderHandlerBuilder {
    pub fn build(handler: OsrRenderHandler) -> RenderHandler {
        Self::new(handler)
    }
}

#[cfg(test)]
mod tests {
    use super::clip_dirty;

    #[test]
    fn empty_means_full_view() {
        assert_eq!(clip_dirty(&[], 800, 600), vec![(0, 0, 800, 600)]);
    }

    #[test]
    fn rects_are_clipped_and_empty_ones_dropped() {
        let rects = [
            (10, 20, 30, 40),
            (-5, -5, 10, 10),
            (790, 590, 50, 50),
            (900, 0, 10, 10),
            (0, 0, 0, 5),
        ];
        assert_eq!(
            clip_dirty(&rects, 800, 600),
            vec![(10, 20, 30, 40), (0, 0, 5, 5), (790, 590, 10, 10)]
        );
    }
}
