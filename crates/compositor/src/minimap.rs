// Screen-space overview when the canvas is zoomed out. Small corner panel
// with page rects + a viewfinder; drag the viewfinder (or click the map)
// to pan. Reuses the HUD shader / solid-quad style.

use crate::browser::Page;
use crate::output::{Rect, Theme};
use crate::viewport::Viewport;
use bytemuck::{Pod, Zeroable};

/// Show when viewport zoom is strictly below this.
pub const SHOW_BELOW: f32 = 0.55;

/// Target size as a fraction of the shorter window side (keeps aspect ~3:2).
const SIZE_FRAC: f32 = 0.22;
const MIN_PANEL_W: f32 = 140.0;
const MAX_PANEL_W: f32 = 420.0;
const ASPECT: f32 = 3.0 / 2.0; // width / height
const MARGIN_FRAC: f32 = 0.018;
const MIN_MARGIN: f32 = 12.0;
const MAX_MARGIN: f32 = 28.0;
const PAD_FRAC: f32 = 0.05;
const OUTLINE: f32 = 1.5;

fn panel_geom(sw: f32, sh: f32) -> (f32, f32, f32, f32, f32) {
    let shorter = sw.min(sh);
    let margin = (shorter * MARGIN_FRAC).clamp(MIN_MARGIN, MAX_MARGIN);
    let mut panel_w = (shorter * SIZE_FRAC).clamp(MIN_PANEL_W, MAX_PANEL_W);
    // Don't eat more than ~28% of width on narrow windows.
    panel_w = panel_w.min(sw * 0.28).max(MIN_PANEL_W.min(sw * 0.4));
    let panel_h = (panel_w / ASPECT).min(sh * 0.28).max(96.0);
    let pad = (panel_w * PAD_FRAC).clamp(6.0, 14.0);
    (panel_w, panel_h, margin, pad, shorter)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MmVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

/// Cached mapping from last `rebuild` for hit-testing / drag.
#[derive(Clone, Copy)]
struct MapSpace {
    panel: Rect,
    /// Where world AABB maps into the panel (letterboxed).
    origin: (f32, f32),
    scale: f32,
    world: Rect,
    viewfinder: Rect,
}

pub struct Minimap {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: u32,
    vertex_count: u32,
    bind_group: wgpu::BindGroup,
    screen_size: (f32, f32),
    space: Option<MapSpace>,
    visible: bool,
}

impl Minimap {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, surface_format: wgpu::TextureFormat) -> Self {
        let texture = solid_white_texture(device, queue);
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("minimap atlas layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("minimap atlas bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&wgpu::TextureViewDescriptor::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("minimap shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("output/hud.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("minimap pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("minimap pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MmVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let vertex_capacity = 512u32;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("minimap verts"),
            size: (vertex_capacity as u64) * std::mem::size_of::<MmVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            vertex_buffer,
            vertex_capacity,
            vertex_count: 0,
            bind_group,
            screen_size: (1.0, 1.0),
            space: None,
            visible: false,
        }
    }

    pub fn set_screen_size(&mut self, w: f32, h: f32) {
        self.screen_size = (w, h);
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Scale factor of the last rebuild (content px per world unit).
    pub fn scale(&self) -> Option<f32> {
        self.space.map(|s| s.scale)
    }

    pub fn contains_screen(&self, x: f32, y: f32) -> bool {
        self.space
            .map(|s| {
                x >= s.panel.x
                    && x <= s.panel.x + s.panel.w
                    && y >= s.panel.y
                    && y <= s.panel.y + s.panel.h
            })
            .unwrap_or(false)
    }

    pub fn hit_viewfinder(&self, x: f32, y: f32) -> bool {
        self.space
            .map(|s| {
                let v = s.viewfinder;
                let pad = 4.0;
                x >= v.x - pad
                    && x <= v.x + v.w + pad
                    && y >= v.y - pad
                    && y <= v.y + v.h + pad
            })
            .unwrap_or(false)
    }

    /// Screen point → world point using the last rebuild mapping.
    pub fn screen_to_world(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let s = self.space?;
        if s.scale <= f32::EPSILON {
            return None;
        }
        Some((
            s.world.x + (x - s.origin.0) / s.scale,
            s.world.y + (y - s.origin.1) / s.scale,
        ))
    }

    pub fn rebuild(
        &mut self,
        queue: &wgpu::Queue,
        device: &wgpu::Device,
        pages: &[Page],
        viewport: &Viewport,
        theme: &Theme,
        focused_index: Option<usize>,
    ) {
        let (sw, sh) = self.screen_size;
        if viewport.zoom >= SHOW_BELOW || sw < 32.0 || sh < 32.0 {
            self.visible = false;
            self.vertex_count = 0;
            self.space = None;
            return;
        }
        self.visible = true;

        let (panel_w, panel_h, margin, pad, _) = panel_geom(sw, sh);
        let panel = Rect {
            x: (sw - panel_w - margin).max(4.0),
            y: (sh - panel_h - margin).max(4.0),
            w: panel_w,
            h: panel_h,
        };
        let inner = Rect {
            x: panel.x + pad,
            y: panel.y + pad,
            w: (panel.w - pad * 2.0).max(1.0),
            h: (panel.h - pad * 2.0).max(1.0),
        };

        let world = world_aabb(pages, viewport, sw, sh);
        let (scale, origin) = fit_aabb(world, inner);

        let view_world = Rect {
            x: viewport.offset.0,
            y: viewport.offset.1,
            w: sw / viewport.zoom,
            h: sh / viewport.zoom,
        };
        let mut viewfinder = Rect {
            x: origin.0 + (view_world.x - world.x) * scale,
            y: origin.1 + (view_world.y - world.y) * scale,
            w: view_world.w * scale,
            h: view_world.h * scale,
        };
        viewfinder = clip_rect(viewfinder, inner);

        self.space = Some(MapSpace {
            panel,
            origin,
            scale,
            world,
            viewfinder,
        });

        let mut verts: Vec<MmVertex> = Vec::new();
        let panel_bg = parse_rgb(theme.help_bg, 0.90);
        let panel_border = parse_rgb(theme.help_card_border, 1.0);
        let page_fill = parse_rgb(theme.help_card_bg, 0.95);
        let page_border = parse_rgb(theme.help_card_border, 1.0);
        let focus = rgba_focus(theme);

        push_rect(
            &mut verts,
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            sw,
            sh,
            panel_bg,
        );
        push_rect_outline(
            &mut verts,
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            sw,
            sh,
            panel_border,
            1.0,
        );

        for (i, page) in pages.iter().enumerate() {
            if page.ephemeral {
                continue;
            }
            let r = page.rect;
            let sx = origin.0 + (r.x - world.x) * scale;
            let sy = origin.1 + (r.y - world.y) * scale;
            let mut pr = Rect {
                x: sx,
                y: sy,
                w: r.w * scale,
                h: r.h * scale,
            };
            pr = clip_rect(pr, inner);
            if pr.w < 1.0 || pr.h < 1.0 {
                continue;
            }
            push_rect(&mut verts, pr.x, pr.y, pr.w, pr.h, sw, sh, page_fill);
            let outline = if Some(i) == focused_index {
                focus
            } else {
                page_border
            };
            push_rect_outline(&mut verts, pr.x, pr.y, pr.w, pr.h, sw, sh, outline, 1.0);
        }

        if viewfinder.w >= 2.0 && viewfinder.h >= 2.0 {
            push_rect_outline(
                &mut verts,
                viewfinder.x,
                viewfinder.y,
                viewfinder.w,
                viewfinder.h,
                sw,
                sh,
                focus,
                OUTLINE,
            );
        }

        self.vertex_count = verts.len() as u32;
        if self.vertex_count > self.vertex_capacity {
            self.vertex_capacity = self.vertex_count.next_power_of_two().max(64);
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("minimap verts"),
                size: (self.vertex_capacity as u64) * std::mem::size_of::<MmVertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !verts.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&verts));
        }
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.vertex_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}

fn world_aabb(pages: &[Page], viewport: &Viewport, sw: f32, sh: f32) -> Rect {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    let mut any = false;
    for page in pages {
        if page.ephemeral {
            continue;
        }
        any = true;
        min_x = min_x.min(page.rect.x);
        min_y = min_y.min(page.rect.y);
        max_x = max_x.max(page.rect.x + page.rect.w);
        max_y = max_y.max(page.rect.y + page.rect.h);
    }
    if !any {
        return Rect {
            x: viewport.offset.0,
            y: viewport.offset.1,
            w: sw / viewport.zoom,
            h: sh / viewport.zoom,
        };
    }
    // Include current view so the viewfinder always has somewhere to sit.
    min_x = min_x.min(viewport.offset.0);
    min_y = min_y.min(viewport.offset.1);
    max_x = max_x.max(viewport.offset.0 + sw / viewport.zoom);
    max_y = max_y.max(viewport.offset.1 + sh / viewport.zoom);
    let pad = ((max_x - min_x).max(max_y - min_y) * 0.06).max(40.0);
    Rect {
        x: min_x - pad,
        y: min_y - pad,
        w: (max_x - min_x + pad * 2.0).max(1.0),
        h: (max_y - min_y + pad * 2.0).max(1.0),
    }
}

fn fit_aabb(world: Rect, inner: Rect) -> (f32, (f32, f32)) {
    let sx = inner.w / world.w;
    let sy = inner.h / world.h;
    let scale = sx.min(sy).max(1e-4);
    let used_w = world.w * scale;
    let used_h = world.h * scale;
    let origin = (
        inner.x + (inner.w - used_w) * 0.5,
        inner.y + (inner.h - used_h) * 0.5,
    );
    (scale, origin)
}

fn clip_rect(r: Rect, clip: Rect) -> Rect {
    let x0 = r.x.max(clip.x);
    let y0 = r.y.max(clip.y);
    let x1 = (r.x + r.w).min(clip.x + clip.w);
    let y1 = (r.y + r.h).min(clip.y + clip.h);
    Rect {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0.0),
        h: (y1 - y0).max(0.0),
    }
}

fn solid_white_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("minimap solid"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[255, 255, 255, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture
}

fn push_rect(out: &mut Vec<MmVertex>, x: f32, y: f32, w: f32, h: f32, sw: f32, sh: f32, color: [f32; 4]) {
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
    let to_ndc = |px: f32, py: f32| -> [f32; 2] {
        [(px / sw) * 2.0 - 1.0, 1.0 - (py / sh) * 2.0]
    };
    let p00 = to_ndc(x0, y0);
    let p10 = to_ndc(x1, y0);
    let p01 = to_ndc(x0, y1);
    let p11 = to_ndc(x1, y1);
    let uv = [0.0, 0.0];
    out.extend_from_slice(&[
        MmVertex {
            pos: p00,
            uv,
            color,
        },
        MmVertex {
            pos: p10,
            uv,
            color,
        },
        MmVertex {
            pos: p11,
            uv,
            color,
        },
        MmVertex {
            pos: p00,
            uv,
            color,
        },
        MmVertex {
            pos: p11,
            uv,
            color,
        },
        MmVertex {
            pos: p01,
            uv,
            color,
        },
    ]);
}

fn push_rect_outline(
    out: &mut Vec<MmVertex>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    sw: f32,
    sh: f32,
    color: [f32; 4],
    t: f32,
) {
    push_rect(out, x, y, w, t, sw, sh, color);
    push_rect(out, x, y + h - t, w, t, sw, sh, color);
    push_rect(out, x, y, t, h, sw, sh, color);
    push_rect(out, x + w - t, y, t, h, sw, sh, color);
}

fn parse_rgb(s: &str, alpha: f32) -> [f32; 4] {
    let inner = s
        .trim()
        .strip_prefix("rgb(")
        .and_then(|t| t.strip_suffix(')'))
        .unwrap_or("128,128,128");
    let mut parts = inner.split(',').filter_map(|p| p.trim().parse::<f32>().ok());
    let r = parts.next().unwrap_or(128.0) / 255.0;
    let g = parts.next().unwrap_or(128.0) / 255.0;
    let b = parts.next().unwrap_or(128.0) / 255.0;
    [r, g, b, alpha]
}

fn rgba_focus(theme: &Theme) -> [f32; 4] {
    let c = theme.focus_border_color;
    [c[0], c[1], c[2], 1.0]
}
