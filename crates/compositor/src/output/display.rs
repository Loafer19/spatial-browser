// The wgpu side of the compositor: window surface, render pipeline, and
// per-page textured quads. Each page owns its own `PageQuad` (small
// vertex buffer) and CEF texture bind group; `GpuState::render` draws
// whichever ones it's handed, back-to-front.

use super::theme::{TOKYO_NIGHT, Theme};
use std::sync::Arc;
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

/// A page's position and size on the canvas, in physical pixels, origin
/// top-left, y-down — the same convention as window/mouse coordinates.
/// The camera is identity for now (canvas space == screen space); pan/
/// zoom would apply a transform here before converting to NDC.
#[derive(Clone, Copy, Debug)]
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
}

// Layout must match `PageStyle` in shader.wgsl field-for-field: every
// field is a plain [f32; 4] specifically so there's no ambiguity around
// WGSL's std140-ish uniform alignment rules (vec2/f32 mixed in would
// need manual padding to hit the same layout naga expects).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PageStyleUniform {
    // xy = page size in pixels, z = corner radius, w = focus border width
    size_radius: [f32; 4],
    border_color: [f32; 4],
    // x = 1.0 if focused, 0.0 otherwise; yzw unused
    focused: [f32; 4],
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
                border_color: [0.0; 4],
                focused: [0.0; 4],
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

    fn update(&self, queue: &wgpu::Queue, rect: Rect, viewport: (f32, f32), focused: bool, theme: &Theme) {
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
                size_radius: [rect.w, rect.h, theme.corner_radius, theme.focus_border_width],
                border_color: theme.focus_border_color,
                focused: [if focused { 1.0 } else { 0.0 }; 4],
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
    pub theme: Theme,
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

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .expect("no suitable GPU adapter");

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
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Page Quad Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Page Quad Pipeline Layout"),
            bind_group_layouts: &[
                Some(&texture_bind_group_layout),
                Some(&style_bind_group_layout),
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

        Self {
            surface,
            device,
            queue,
            config,
            window,
            pipeline,
            texture_bind_group_layout,
            style_bind_group_layout,
            theme: TOKYO_NIGHT,
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

    /// Draws `pages` in the given order (back-to-front — last is topmost).
    pub fn render(&mut self, pages: &[PageDraw<'_>]) -> FrameOutcome {
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

        let viewport = (self.config.width as f32, self.config.height as f32);
        for page in pages {
            page.quad.update(
                &self.queue,
                page.rect,
                viewport,
                page.focused,
                &self.theme,
            );
        }

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
                        load: wgpu::LoadOp::Clear(self.theme.canvas_background),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.pipeline);
            for page in pages {
                let Some(bind_group) = page.texture else {
                    continue;
                };
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_bind_group(1, &page.quad.style_bind_group, &[]);
                pass.set_vertex_buffer(0, page.quad.vertex_buffer.slice(..));
                pass.draw(0..4, 0..1);
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
