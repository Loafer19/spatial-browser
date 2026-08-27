// The wgpu side of the compositor: window surface, render pipeline, and
// per-page textured quads. Each page owns its own `PageQuad` (small
// vertex buffer) and CEF texture bind group; `GpuState::render` draws
// whichever ones it's handed, back-to-front.

use super::theme::Theme;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use winit::window::Window;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];

    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// A page's position and size, origin top-left, y-down — the same
/// convention as window/mouse coordinates. Lives in world space (see
/// viewport.rs for the pan/zoom mapping to screen space); `GpuState::render`
/// is handed already screen-space rects.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// Clamps width/height to `max`. A page's rect drives CEF's actual
    /// OSR buffer resolution 1:1 (browser::Page), and wgpu aborts the
    /// whole process on any texture exceeding its dimension limit — no
    /// graceful fallback — so this has to be enforced wherever a rect
    /// reaches a page, not trusted to whatever produced it (dividing a
    /// screen size by a small viewport zoom, e.g. zoom-to-canvas while
    /// zoomed way out, easily produces a world size in the tens of
    /// thousands of pixels).
    pub fn clamp_size(mut self, max: f32) -> Self {
        self.w = self.w.min(max);
        self.h = self.h.min(max);
        self
    }
}

// Layout must match `PageStyle` in shader.wgsl field-for-field: every
// field is a plain [f32; 4] specifically so there's no ambiguity around
// WGSL's std140-ish uniform alignment rules (vec2/f32 mixed in would
// need manual padding to hit the same layout naga expects).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PageStyleUniform {
    // xy = page size in pixels, z = corner radius,
    // w = 1.0 if no CEF frame yet (placeholder)
    size_radius: [f32; 4],
    focus_border_color: [f32; 4],
    unfocused_border_color: [f32; 4],
    // x = 1.0 if focused else 0.0, y = focus border width,
    // z = unfocused border width, w = unfocused dim factor
    flags: [f32; 4],
    // x = load progress 0..1, y = 1.0 if top load bar visible,
    // z = bar height in page pixels (from theme border width)
    load: [f32; 4],
}

/// One page's on-GPU quad geometry and chrome style (rounded corners,
/// focus ring): its own small vertex buffer and style uniform buffer,
/// rather than one shared buffer rewritten per page per frame. wgpu's
/// queue writes aren't ordered against this frame's draw calls that way —
/// with one shared buffer, the last page's `write_buffer` would win for
/// every draw call in the same submission, not just its own.
pub struct PageQuad {
    vertex_buffer: wgpu::Buffer,
    style_buffer: wgpu::Buffer,
    style_bind_group: wgpu::BindGroup,
}

impl PageQuad {
    pub fn new(device: &wgpu::Device, style_bind_group_layout: &wgpu::BindGroupLayout) -> Self {
        use wgpu::util::DeviceExt;
        let zero = Vertex {
            position: [0.0; 3],
            tex_coords: [0.0; 2],
        };
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Page Quad Vertex Buffer"),
            contents: bytemuck::cast_slice(&[zero; 4]),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        let style_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Page Style Buffer"),
            // Overwritten by `update` before the first real draw of this
            // page, so the initial border color here doesn't matter.
            contents: bytemuck::cast_slice(&[PageStyleUniform {
                size_radius: [0.0; 4],
                focus_border_color: [0.0; 4],
                unfocused_border_color: [0.0; 4],
                flags: [0.0; 4],
                load: [0.0; 4],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let style_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Page Style Bind Group"),
            layout: style_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: style_buffer.as_entire_binding(),
            }],
        });

        Self {
            vertex_buffer,
            style_buffer,
            style_bind_group,
        }
    }

    fn update(
        &self,
        queue: &wgpu::Queue,
        rect: Rect,
        viewport: (f32, f32),
        focused: bool,
        is_loading: bool,
        load_progress: f32,
        load_bar: bool,
        theme: &Theme,
    ) {
        let (vw, vh) = viewport;
        let left = (rect.x / vw) * 2.0 - 1.0;
        let right = ((rect.x + rect.w) / vw) * 2.0 - 1.0;
        let top = 1.0 - (rect.y / vh) * 2.0;
        let bottom = 1.0 - ((rect.y + rect.h) / vh) * 2.0;

        let vertices = [
            Vertex {
                position: [left, top, 0.0],
                tex_coords: [0.0, 0.0],
            },
            Vertex {
                position: [right, top, 0.0],
                tex_coords: [1.0, 0.0],
            },
            Vertex {
                position: [left, bottom, 0.0],
                tex_coords: [0.0, 1.0],
            },
            Vertex {
                position: [right, bottom, 0.0],
                tex_coords: [1.0, 1.0],
            },
        ];
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));

        queue.write_buffer(
            &self.style_buffer,
            0,
            bytemuck::cast_slice(&[PageStyleUniform {
                size_radius: [
                    rect.w,
                    rect.h,
                    theme.corner_radius,
                    if is_loading { 1.0 } else { 0.0 },
                ],
                focus_border_color: theme.focus_border_color,
                unfocused_border_color: theme.unfocused_border_color,
                flags: [
                    if focused { 1.0 } else { 0.0 },
                    theme.focus_border_width,
                    theme.unfocused_border_width,
                    theme.unfocused_dim,
                ],
                load: [
                    load_progress.clamp(0.0, 1.0),
                    if load_bar { 1.0 } else { 0.0 },
                    // Match focus ring weight so Tokyo Night (4px) vs ANSI
                    // (1.5px) get a proportional top bar.
                    theme.focus_border_width.clamp(2.0, 5.0),
                    0.0,
                ],
            }]),
        );
    }
}

/// One page's draw call for a single frame: where it goes (`rect`) and
/// what to paint into it (`texture` — `None` while CEF hasn't produced a
/// first frame yet, in which case the page is skipped for this draw).
pub struct PageDraw<'a> {
    pub rect: Rect,
    pub quad: &'a PageQuad,
    pub texture: Option<&'a wgpu::BindGroup>,
    pub focused: bool,
    /// CEF load progress 0..1 while the top bar should show.
    pub load_progress: f32,
    pub load_bar: bool,
}

pub struct GpuState {
    surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pub window: Arc<Window>,
    pipeline: wgpu::RenderPipeline,
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
    pub style_bind_group_layout: wgpu::BindGroupLayout,
    // Stand-in for group 0 (texture+sampler) on a page with no CEF frame
    // yet — the pipeline layout requires *something* bound there even
    // though the loading-placeholder fragment path (shader.wgsl) never
    // samples it.
    placeholder_bind_group: wgpu::BindGroup,
    // Elapsed-seconds uniform shared by every page this frame, purely to
    // drive the loading-placeholder pulse — its own bind group rather
    // than duplicated per-page style data.
    frame_globals_buffer: wgpu::Buffer,
    frame_globals_bind_group: wgpu::BindGroup,
    start: Instant,
    // The dot-grid background (background.wgsl): its own pipeline since
    // it's a fullscreen triangle with no vertex buffer and a different
    // uniform shape, not a variant of the page-quad pipeline.
    background_pipeline: wgpu::RenderPipeline,
    background_buffer: wgpu::Buffer,
    background_bind_group: wgpu::BindGroup,
}

impl GpuState {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        // CEF's shared-texture OSR export requires the Vulkan external-memory
        // extensions specifically; `Instance::default()` doesn't guarantee
        // Vulkan gets picked (or picked with the right extension set), which
        // silently produces an importable-but-empty (black) texture instead
        // of an error. Force the backend that's proven to carry the shared
        // texture's actual contents.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_comma_list("vulkan"),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(window.clone())
            .expect("create wgpu surface");

        // Prefer LowPower by default so wgpu lands on the same GPU CEF's
        // GPU process typically uses (the display-driving iGPU). On
        // hybrid laptops HighPerformance picks the discrete GPU while
        // Chromium stays on the iGPU — DMA-BUF shared-texture import then
        // fails across devices. Override with SPATIAL_BROWSER_GPU=high
        // (and usually SPATIAL_BROWSER_OSR=cpu; see osr_shared_texture_enabled).
        let power_preference = gpu_power_preference();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .expect("no suitable GPU adapter");
        log::info!(
            "wgpu adapter: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().device_type
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_non_sampler_bindings: 2048,
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .expect("failed to create device");

        let caps = surface.get_capabilities(&adapter);
        // Exactly Bgra8Unorm, not just "any non-sRGB format": the CEF
        // paint texture (cef-bridge's on_paint) is plain Bgra8Unorm
        // holding CEF's raw already-encoded bytes, and this GPU's
        // capability list order happens to put an HDR float format
        // (Rgba16Float) before it — matching on "first non-sRGB" picked
        // that instead, and linear float values presented without a
        // gamma pass came out visibly washed out/light. An sRGB surface
        // format has the opposite problem (double gamma-encodes CEF's
        // bytes on write). Bgra8Unorm is a plain passthrough matching
        // the source texture format exactly.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == wgpu::TextureFormat::Bgra8Unorm)
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Page Texture Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let style_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Page Style Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Group 2 for the page-quad pipeline: elapsed time, shared by
        // every page, used only to animate the loading-placeholder pulse.
        let frame_globals_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Frame Globals Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let frame_globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame Globals Buffer"),
            size: std::mem::size_of::<[f32; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Frame Globals Bind Group"),
            layout: &frame_globals_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_globals_buffer.as_entire_binding(),
            }],
        });

        // A 1x1 opaque texture bound in place of a loading page's (still
        // absent) real one — see `placeholder_bind_group`'s field comment.
        let placeholder_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Placeholder Texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let placeholder_view =
            placeholder_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let placeholder_sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let placeholder_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Placeholder Bind Group"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&placeholder_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&placeholder_sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Page Quad Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Page Quad Pipeline Layout"),
            bind_group_layouts: &[
                Some(&texture_bind_group_layout),
                Some(&style_bind_group_layout),
                Some(&frame_globals_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Page Quad Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(Vertex::desc())],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Alpha blending, not REPLACE: rounded corners work by
                    // making the fragment shader output alpha=0 outside the
                    // rounded rect, which needs real blending against
                    // whatever's already in the framebuffer (the canvas
                    // background, or a page drawn earlier) to look right.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        let background_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Background Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let background_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Background Globals Buffer"),
            size: std::mem::size_of::<[[f32; 4]; 3]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let background_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Background Bind Group"),
            layout: &background_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: background_buffer.as_entire_binding(),
            }],
        });

        let background_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Background Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("background.wgsl").into()),
        });
        let background_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Background Pipeline Layout"),
                bind_group_layouts: &[Some(&background_bind_group_layout)],
                immediate_size: 0,
            });
        let background_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Background Pipeline"),
            layout: Some(&background_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &background_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &background_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Self {
            surface,
            device,
            queue,
            config,
            window,
            pipeline,
            texture_bind_group_layout,
            placeholder_bind_group,
            frame_globals_buffer,
            frame_globals_bind_group,
            start: Instant::now(),
            background_pipeline,
            background_buffer,
            background_bind_group,
            style_bind_group_layout,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn size(&self) -> (f32, f32) {
        (self.config.width as f32, self.config.height as f32)
    }

    /// Draws `pages` in the given order (back-to-front — last is topmost),
    /// over the dot-grid background. `viewport_offset`/`viewport_zoom` are
    /// the canvas pan/zoom (session::Session::viewport) — needed here
    /// only so the background grid can be drawn in world space; page
    /// rects arrive in `pages` already screen-space (see PageDraw).
    /// Optional `hud` is drawn last (screen-space chip strip).
    pub fn render(
        &mut self,
        pages: &[PageDraw<'_>],
        theme: &Theme,
        viewport_offset: (f32, f32),
        viewport_zoom: f32,
        hud: Option<&crate::hud::Hud>,
    ) -> FrameOutcome {
        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return FrameOutcome::Skip;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return FrameOutcome::Reconfigure;
            }
            wgpu::CurrentSurfaceTexture::Validation => return FrameOutcome::Fatal,
        };

        let screen_size = (self.config.width as f32, self.config.height as f32);
        for page in pages {
            page.quad.update(
                &self.queue,
                page.rect,
                screen_size,
                page.focused,
                page.texture.is_none(),
                page.load_progress,
                page.load_bar,
                theme,
            );
        }

        self.queue.write_buffer(
            &self.frame_globals_buffer,
            0,
            bytemuck::cast_slice(&[[self.start.elapsed().as_secs_f32(), 0.0, 0.0, 0.0]]),
        );
        self.queue.write_buffer(
            &self.background_buffer,
            0,
            bytemuck::cast_slice(&[
                [viewport_offset.0, viewport_offset.1, viewport_zoom, 0.0],
                theme.background_dot_color,
                [
                    theme.background_grid_spacing,
                    theme.background_dot_radius,
                    0.0,
                    0.0,
                ],
            ]),
        );

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("canvas encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("page quad pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(theme.canvas_background),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.background_pipeline);
            pass.set_bind_group(0, &self.background_bind_group, &[]);
            pass.draw(0..3, 0..1);

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(2, &self.frame_globals_bind_group, &[]);
            for page in pages {
                let bind_group = page.texture.unwrap_or(&self.placeholder_bind_group);
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_bind_group(1, &page.quad.style_bind_group, &[]);
                pass.set_vertex_buffer(0, page.quad.vertex_buffer.slice(..));
                pass.draw(0..4, 0..1);
            }
            if let Some(hud) = hud {
                hud.draw(&mut pass);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(surface_texture);
        FrameOutcome::Rendered
    }
}

pub enum FrameOutcome {
    Rendered,
    Skip,
    Reconfigure,
    Fatal,
}

/// wgpu power preference for CEF shared-texture compatibility.
///
/// Default is `LowPower` so hybrid laptops pick the iGPU Chromium's GPU
/// process also tends to use. `SPATIAL_BROWSER_GPU=high` / `discrete`
/// forces `HighPerformance` (usually the dGPU) — pair with
/// `SPATIAL_BROWSER_OSR=cpu` unless both processes are forced onto that
/// same device some other way.
fn gpu_power_preference() -> wgpu::PowerPreference {
    match std::env::var("SPATIAL_BROWSER_GPU").ok().as_deref() {
        Some("high" | "discrete" | "dgpu") => wgpu::PowerPreference::HighPerformance,
        Some("low" | "integrated" | "igpu") => wgpu::PowerPreference::LowPower,
        Some(other) => {
            log::warn!("unknown SPATIAL_BROWSER_GPU={other:?}, using low (iGPU-friendly)");
            wgpu::PowerPreference::LowPower
        }
        None => wgpu::PowerPreference::LowPower,
    }
}

/// Whether new CEF browsers should request GPU shared-texture OSR.
///
/// On when the `accelerated_osr` feature is compiled in, unless
/// `SPATIAL_BROWSER_OSR=cpu` forces the CPU `on_paint` path. Also auto-
/// disables when the user asked for the discrete GPU via
/// `SPATIAL_BROWSER_GPU=high` without also setting `SPATIAL_BROWSER_OSR=gpu`,
/// because cross-device DMA-BUF import is what made shared texture unusable
/// on hybrid laptops in the first place.
pub fn osr_shared_texture_enabled() -> bool {
    #[cfg(not(feature = "accelerated_osr"))]
    {
        return false;
    }
    #[cfg(feature = "accelerated_osr")]
    {
        match std::env::var("SPATIAL_BROWSER_OSR").ok().as_deref() {
            Some("cpu" | "software") => false,
            Some("gpu" | "shared" | "accelerated") => true,
            Some(other) => {
                log::warn!("unknown SPATIAL_BROWSER_OSR={other:?}, using feature default");
                !discrete_gpu_without_forced_osr()
            }
            None => !discrete_gpu_without_forced_osr(),
        }
    }
}

#[cfg(feature = "accelerated_osr")]
fn discrete_gpu_without_forced_osr() -> bool {
    matches!(
        std::env::var("SPATIAL_BROWSER_GPU").ok().as_deref(),
        Some("high" | "discrete" | "dgpu")
    )
}
