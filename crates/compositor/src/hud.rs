// Top workspace chip strip: reveal on top hit-band, auto-hide on leave.

use crate::output::Theme;
use crate::persistence::workspaces::WorkspaceStore;
use bytemuck::{Pod, Zeroable};
use font8x8::UnicodeFonts;
use std::time::{Duration, Instant};

const REVEAL_BAND: f32 = 14.0;
const HIDE_DELAY: Duration = Duration::from_millis(500);
const CHIP_W: f32 = 44.0;
const CHIP_H: f32 = 36.0;
const CHIP_GAP: f32 = 10.0;
/// Equal inset from the wrapping panel edge to the chip row on every side.
const PANEL_PAD: f32 = 12.0;
/// Gap from the physical top of the window to the panel.
const PANEL_TOP: f32 = 8.0;
const STRIP_HEIGHT: f32 = PANEL_TOP + CHIP_H + PANEL_PAD * 2.0 + 4.0;
const GLYPH_SCALE: f32 = 3.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HudVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

pub enum HudHit {
    Slot(u32),
    Add,
}

pub struct Hud {
    visible: bool,
    pointer_in_strip: bool,
    hide_at: Option<Instant>,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: u32,
    vertex_count: u32,
    atlas_bind_group: wgpu::BindGroup,
    screen_size: (f32, f32),
}

impl Hud {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, surface_format: wgpu::TextureFormat) -> Self {
        let atlas = build_atlas(device, queue);
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hud atlas layout"),
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
        let atlas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hud atlas bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &atlas.create_view(&wgpu::TextureViewDescriptor::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hud shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("output/hud.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hud pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hud pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<HudVertex>() as u64,
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

        let vertex_capacity = 256u32;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hud verts"),
            size: (vertex_capacity as u64) * std::mem::size_of::<HudVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            visible: false,
            pointer_in_strip: false,
            hide_at: None,
            pipeline,
            vertex_buffer,
            vertex_capacity,
            vertex_count: 0,
            atlas_bind_group,
            screen_size: (1.0, 1.0),
        }
    }

    pub fn set_screen_size(&mut self, w: f32, h: f32) {
        self.screen_size = (w, h);
    }

    /// Cursor moved — update reveal / hide timer. `y` in physical px.
    pub fn on_cursor(&mut self, y: f32) {
        let in_band = y <= REVEAL_BAND || (self.visible && y <= STRIP_HEIGHT + 4.0);
        if in_band {
            self.visible = true;
            self.pointer_in_strip = y <= STRIP_HEIGHT + 4.0;
            self.hide_at = None;
        } else if self.visible {
            self.pointer_in_strip = false;
            if self.hide_at.is_none() {
                self.hide_at = Some(Instant::now() + HIDE_DELAY);
            }
        }
    }

    pub fn tick(&mut self) {
        if let Some(t) = self.hide_at {
            if Instant::now() >= t && !self.pointer_in_strip {
                self.visible = false;
                self.hide_at = None;
            }
        }
    }

    fn chips_width(store: &WorkspaceStore) -> f32 {
        let n = store.slot_count() as f32 + 1.0; // slots + plus
        n * CHIP_W + (n - 1.0) * CHIP_GAP
    }

    /// Panel rect and chip-row origin — panel is centered; chips sit with
    /// equal `PANEL_PAD` on left/right/top/bottom inside it.
    fn layout(store: &WorkspaceStore, screen_w: f32) -> (f32, f32, f32, f32, f32, f32) {
        let chips_w = Self::chips_width(store);
        let panel_w = chips_w + PANEL_PAD * 2.0;
        let panel_h = CHIP_H + PANEL_PAD * 2.0;
        let panel_x = ((screen_w - panel_w) * 0.5).max(4.0);
        let panel_y = PANEL_TOP;
        let chip_x = panel_x + PANEL_PAD;
        let chip_y = panel_y + PANEL_PAD;
        (panel_x, panel_y, panel_w, panel_h, chip_x, chip_y)
    }

    pub fn hit_test(&self, store: &WorkspaceStore, x: f32, y: f32) -> Option<HudHit> {
        if !self.visible || y > STRIP_HEIGHT {
            return None;
        }
        let (_, _, _, _, start, cy) = Self::layout(store, self.screen_size.0);
        let mut cx = start;
        for slot in &store.slots {
            if x >= cx && x <= cx + CHIP_W && y >= cy && y <= cy + CHIP_H {
                return Some(HudHit::Slot(slot.id));
            }
            cx += CHIP_W + CHIP_GAP;
        }
        if x >= cx && x <= cx + CHIP_W && y >= cy && y <= cy + CHIP_H {
            return Some(HudHit::Add);
        }
        None
    }

    pub fn rebuild(
        &mut self,
        queue: &wgpu::Queue,
        device: &wgpu::Device,
        store: &WorkspaceStore,
        theme: &Theme,
    ) {
        if !self.visible {
            self.vertex_count = 0;
            return;
        }
        let (sw, sh) = self.screen_size;
        let mut verts: Vec<HudVertex> = Vec::new();
        let (panel_x, panel_y, panel_w, panel_h, start, cy) = Self::layout(store, sw);

        let panel_bg = parse_rgb(theme.help_bg, 0.88);
        let panel_border = parse_rgb(theme.help_card_border, 1.0);
        let accent = rgba_from_theme_focus(theme);
        let idle = parse_rgb(theme.help_card_bg, 0.96);
        let border = parse_rgb(theme.help_card_border, 1.0);
        let glyph_on = parse_rgb(theme.help_key_fg, 1.0);
        let glyph_off = parse_rgb(theme.help_fg, 1.0);

        push_rect(
            &mut verts,
            panel_x,
            panel_y,
            panel_w,
            panel_h,
            sw,
            sh,
            panel_bg,
            atlas_uv_solid(),
        );
        push_rect_outline(
            &mut verts,
            panel_x,
            panel_y,
            panel_w,
            panel_h,
            sw,
            sh,
            panel_border,
        );

        let mut cx = start;
        for slot in &store.slots {
            let active = slot.id == store.active;
            let bg = if active { accent } else { idle };
            let gc = if active { glyph_on } else { glyph_off };
            push_rect(&mut verts, cx, cy, CHIP_W, CHIP_H, sw, sh, bg, atlas_uv_solid());
            push_rect_outline(&mut verts, cx, cy, CHIP_W, CHIP_H, sw, sh, border);
            let label = if slot.id >= 10 {
                format!("{}", slot.id)
            } else {
                std::char::from_digit(slot.id, 10)
                    .unwrap_or('?')
                    .to_string()
            };
            push_glyph_centered(&mut verts, &label, cx, cy, CHIP_W, CHIP_H, sw, sh, gc);
            cx += CHIP_W + CHIP_GAP;
        }
        push_rect(&mut verts, cx, cy, CHIP_W, CHIP_H, sw, sh, idle, atlas_uv_solid());
        push_rect_outline(&mut verts, cx, cy, CHIP_W, CHIP_H, sw, sh, border);
        push_glyph_centered(&mut verts, "+", cx, cy, CHIP_W, CHIP_H, sw, sh, glyph_off);

        self.vertex_count = verts.len() as u32;
        if self.vertex_count > self.vertex_capacity {
            self.vertex_capacity = self.vertex_count.next_power_of_two().max(64);
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("hud verts"),
                size: (self.vertex_capacity as u64) * std::mem::size_of::<HudVertex>() as u64,
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
        pass.set_bind_group(0, &self.atlas_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}

fn atlas_uv_solid() -> [f32; 4] {
    // last cell of atlas is solid white
    let i = 11.0;
    let u0 = i / 12.0;
    [u0, 0.0, u0 + 1.0 / 12.0, 1.0]
}

fn glyph_uv(ch: char) -> [f32; 4] {
    let idx = match ch {
        '0'..='9' => (ch as u8 - b'0') as f32,
        '+' => 10.0,
        _ => 11.0,
    };
    let u0 = idx / 12.0;
    [u0, 0.0, u0 + 1.0 / 12.0, 1.0]
}

/// Ink bounds of a font8x8 glyph inside its 8×8 cell (inclusive).
fn glyph_ink(ch: char) -> Option<(u32, u32, u32, u32)> {
    let bitmap = font8x8::BASIC_FONTS.get(ch)?;
    let mut min_x = 8u32;
    let mut min_y = 8u32;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for (row, byte) in bitmap.iter().enumerate() {
        for bit in 0..8u32 {
            if byte & (1 << bit) != 0 {
                any = true;
                min_x = min_x.min(bit);
                max_x = max_x.max(bit);
                min_y = min_y.min(row as u32);
                max_y = max_y.max(row as u32);
            }
        }
    }
    if !any {
        return None;
    }
    Some((min_x, min_y, max_x, max_y))
}

fn push_glyph_centered(
    out: &mut Vec<HudVertex>,
    text: &str,
    cx: f32,
    cy: f32,
    cw: f32,
    ch: f32,
    sw: f32,
    sh: f32,
    color: [f32; 4],
) {
    let scale = GLYPH_SCALE;
    // Measure the combined ink box across all characters so we center what
    // you actually see, not the empty padding inside each 8×8 cell.
    let mut ink_min_x = f32::MAX;
    let mut ink_min_y = f32::MAX;
    let mut ink_max_x = f32::MIN;
    let mut ink_max_y = f32::MIN;
    let mut cell_x = 0.0f32;
    for c in text.chars() {
        if let Some((x0, y0, x1, y1)) = glyph_ink(c) {
            ink_min_x = ink_min_x.min(cell_x + x0 as f32);
            ink_max_x = ink_max_x.max(cell_x + x1 as f32 + 1.0);
            ink_min_y = ink_min_y.min(y0 as f32);
            ink_max_y = ink_max_y.max(y1 as f32 + 1.0);
        }
        cell_x += 8.0;
    }
    if !ink_min_x.is_finite() {
        return;
    }
    let tw = (ink_max_x - ink_min_x) * scale;
    let th = (ink_max_y - ink_min_y) * scale;
    // Top-left of the (virtual) combined cell space, chosen so the ink box
    // is centered in the chip.
    let base_x = cx + (cw - tw) * 0.5 - ink_min_x * scale;
    let base_y = cy + (ch - th) * 0.5 - ink_min_y * scale;

    cell_x = 0.0;
    for c in text.chars() {
        let uv = glyph_uv(c);
        push_rect(
            out,
            base_x + cell_x * scale,
            base_y,
            8.0 * scale,
            8.0 * scale,
            sw,
            sh,
            color,
            uv,
        );
        cell_x += 8.0;
    }
}

fn push_rect(
    out: &mut Vec<HudVertex>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    sw: f32,
    sh: f32,
    color: [f32; 4],
    uv: [f32; 4],
) {
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
    let (u0, v0, u1, v1) = (uv[0], uv[1], uv[2], uv[3]);
    let to_ndc = |px: f32, py: f32| -> [f32; 2] {
        [
            (px / sw) * 2.0 - 1.0,
            1.0 - (py / sh) * 2.0,
        ]
    };
    let p00 = to_ndc(x0, y0);
    let p10 = to_ndc(x1, y0);
    let p01 = to_ndc(x0, y1);
    let p11 = to_ndc(x1, y1);
    out.extend_from_slice(&[
        HudVertex {
            pos: p00,
            uv: [u0, v0],
            color,
        },
        HudVertex {
            pos: p10,
            uv: [u1, v0],
            color,
        },
        HudVertex {
            pos: p11,
            uv: [u1, v1],
            color,
        },
        HudVertex {
            pos: p00,
            uv: [u0, v0],
            color,
        },
        HudVertex {
            pos: p11,
            uv: [u1, v1],
            color,
        },
        HudVertex {
            pos: p01,
            uv: [u0, v1],
            color,
        },
    ]);
}

fn push_rect_outline(
    out: &mut Vec<HudVertex>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    sw: f32,
    sh: f32,
    color: [f32; 4],
) {
    let t = 1.0;
    push_rect(out, x, y, w, t, sw, sh, color, atlas_uv_solid());
    push_rect(out, x, y + h - t, w, t, sw, sh, color, atlas_uv_solid());
    push_rect(out, x, y, t, h, sw, sh, color, atlas_uv_solid());
    push_rect(out, x + w - t, y, t, h, sw, sh, color, atlas_uv_solid());
}

fn build_atlas(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    // 12 cells × 8 px: digits 0-9, '+', solid white
    let cell = 8u32;
    let cols = 12u32;
    let width = cell * cols;
    let height = cell;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for d in 0..10u8 {
        blit_glyph(&mut pixels, width, d as u32, char::from(b'0' + d));
    }
    blit_glyph(&mut pixels, width, 10, '+');
    // solid white cell 11
    for y in 0..cell {
        for x in 0..cell {
            let i = ((y * width + 11 * cell + x) * 4) as usize;
            pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hud atlas"),
        size: wgpu::Extent3d {
            width,
            height,
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
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    texture
}

fn blit_glyph(pixels: &mut [u8], width: u32, col: u32, ch: char) {
    let Some(bitmap) = font8x8::BASIC_FONTS.get(ch) else {
        return;
    };
    let cell = 8u32;
    for (row, byte) in bitmap.iter().enumerate() {
        for bit in 0..8u32 {
            if byte & (1 << bit) != 0 {
                let x = col * cell + bit;
                let y = row as u32;
                let i = ((y * width + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
}

fn parse_rgb(s: &str, alpha: f32) -> [f32; 4] {
    // "rgb(r,g,b)"
    let inner = s
        .trim()
        .trim_start_matches("rgb(")
        .trim_end_matches(')');
    let mut parts = inner.split(',').filter_map(|p| p.trim().parse::<f32>().ok());
    let r = parts.next().unwrap_or(128.0) / 255.0;
    let g = parts.next().unwrap_or(128.0) / 255.0;
    let b = parts.next().unwrap_or(128.0) / 255.0;
    [r, g, b, alpha]
}

fn rgba_from_theme_focus(theme: &Theme) -> [f32; 4] {
    let c = theme.focus_border_color;
    [c[0], c[1], c[2], 0.95]
}
