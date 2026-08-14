// Compositor: owns the window, the GPU surface, and the spatial canvas.
// This step: one CEF page, off-screen rendered into a shared GPU texture
// (no CPU copy), drawn as a single full-window textured quad. Multiple
// pages arranged on a pannable/zoomable canvas is the next step; for now
// there's exactly one quad covering the whole surface.
//
// CEF's multi-process model means this same binary is re-exec'd as the
// renderer/gpu/utility helper processes, so the CEF bootstrap (execute_process
// / initialize) has to run at the very top of main(), before any window or
// wgpu setup, and the winit loop has to cooperatively pump CEF's message
// loop (do_message_loop_work) instead of blocking in `run_app`.

mod input;

use cef::{args::Args, *};
use cef_bridge::{
    AppBuilder, ClientBuilder, OsrApp, OsrRenderHandler, OsrRequestContextHandler,
    RequestContextHandlerBuilder, TEXTURE,
};
use input::MouseInput;
use std::{cell::RefCell, process::ExitCode, rc::Rc, sync::Arc, thread::sleep, time::Duration};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::pump_events::{EventLoopExtPumpEvents, PumpStatus},
    window::{Window, WindowAttributes, WindowId},
};

const HOME_PAGE: &str = "https://example.com";

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

struct Quad {
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
}

impl Quad {
    fn full_screen(device: &wgpu::Device) -> Self {
        use wgpu::util::DeviceExt;

        let vertices = [
            Vertex {
                position: [-1.0, 1.0, 0.0],
                tex_coords: [0.0, 0.0],
            },
            Vertex {
                position: [1.0, 1.0, 0.0],
                tex_coords: [1.0, 0.0],
            },
            Vertex {
                position: [-1.0, -1.0, 0.0],
                tex_coords: [0.0, 1.0],
            },
            Vertex {
                position: [1.0, -1.0, 0.0],
                tex_coords: [1.0, 1.0],
            },
        ];

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Page Quad Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            vertex_buffer,
            vertex_count: vertices.len() as u32,
        }
    }
}

struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    window: Arc<Window>,
    pipeline: wgpu::RenderPipeline,
    quad: Quad,
    texture_bind_group_layout: wgpu::BindGroupLayout,
}

impl GpuState {
    async fn new(window: Arc<Window>) -> Self {
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Page Quad Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Page Quad Pipeline Layout"),
            bind_group_layouts: &[Some(&texture_bind_group_layout)],
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
                    blend: Some(wgpu::BlendState::REPLACE),
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

        let quad = Quad::full_screen(&device);

        Self {
            surface,
            device,
            queue,
            config,
            window,
            pipeline,
            quad,
            texture_bind_group_layout,
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn render(&mut self) -> FrameOutcome {
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
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.05,
                            b: 0.08,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            TEXTURE.with_borrow(|texture| {
                if let Some(bind_group) = texture.as_ref() {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, bind_group, &[]);
                    pass.set_vertex_buffer(0, self.quad.vertex_buffer.slice(..));
                    pass.draw(0..self.quad.vertex_count, 0..1);
                }
            });
        }
        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(surface_texture);
        FrameOutcome::Rendered
    }
}

enum FrameOutcome {
    Rendered,
    Skip,
    Reconfigure,
    Fatal,
}

struct BrowserState {
    browser: cef::Browser,
    size: Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
}

#[derive(Default)]
struct App {
    state: Option<GpuState>,
    browser: Option<BrowserState>,
    mouse: MouseInput,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(WindowAttributes::default().with_title("spatial-browser"))
                .expect("failed to create window"),
        );
        let state = pollster::block_on(GpuState::new(window.clone()));

        // Shared-texture (GPU) OSR needs the CEF GPU process's DMA-BUF export
        // and our wgpu Vulkan import to land on the same physical GPU. On a
        // hybrid-graphics laptop, Chromium's GPU process defaults to the
        // display-driving iGPU while wgpu's HighPerformance pick can land on
        // the discrete GPU; Chromium also strips most env vars from its
        // child processes, so there's no reliable way to force both onto
        // the same device from here. CPU OSR (on_paint, plain memcpy) has no
        // such cross-device requirement, so that's what's wired up for now.
        let window_info = WindowInfo {
            windowless_rendering_enabled: true as _,
            shared_texture_enabled: false as _,
            external_begin_frame_enabled: true as _,
            ..Default::default()
        };

        let device_scale_factor = window.scale_factor();
        let (render_handler, browser_size) = OsrRenderHandler::new(
            state.device.clone(),
            state.queue.clone(),
            state.texture_bind_group_layout.clone(),
            device_scale_factor as _,
            window.inner_size().to_logical(device_scale_factor),
        );

        let browser_settings = BrowserSettings {
            windowless_frame_rate: 60,
            ..Default::default()
        };
        let mut context = cef::request_context_create_context(
            Some(&RequestContextSettings::default()),
            Some(&mut RequestContextHandlerBuilder::build(
                OsrRequestContextHandler {},
            )),
        );

        let browser = cef::browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut ClientBuilder::build(render_handler)),
            Some(&HOME_PAGE.into()),
            Some(&browser_settings),
            None,
            context.as_mut(),
        )
        .expect("failed to create CEF browser");

        self.browser.replace(BrowserState {
            browser,
            size: browser_size,
        });
        self.state = Some(state);
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        if state.window.id() != id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);
                if let Some(browser) = &self.browser {
                    *browser.size.borrow_mut() = size.to_logical(state.window.scale_factor());
                    if let Some(host) = browser.browser.host() {
                        host.was_resized();
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = state.window.scale_factor();
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse
                    .cursor_moved((position.x, position.y), scale, host.as_ref());
            }
            WindowEvent::CursorLeft { .. } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse.cursor_left(host.as_ref());
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse.button(element_state, button, host.as_ref());
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let host = self.browser.as_ref().and_then(|b| b.browser.host());
                self.mouse.wheel(delta, host.as_ref());
            }
            WindowEvent::Focused(focused) => {
                if let Some(host) = self.browser.as_ref().and_then(|b| b.browser.host()) {
                    host.set_focus(focused as _);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(host) = self.browser.as_ref().and_then(|b| b.browser.host()) {
                    host.send_external_begin_frame();
                }
                match state.render() {
                    FrameOutcome::Rendered | FrameOutcome::Skip => {}
                    FrameOutcome::Reconfigure => {
                        let size = state.window.inner_size();
                        state.resize(size.width, size.height);
                    }
                    FrameOutcome::Fatal => {
                        log::error!("fatal surface validation error, exiting");
                        event_loop.exit();
                    }
                }
                state.window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let cmd = args.as_cmd_line().unwrap();
    let is_browser_process = cmd.has_switch(Some(&"type".into())) != 1;

    let mut app = AppBuilder::build(OsrApp::new());
    let ret = execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());

    if is_browser_process {
        assert!(ret == -1, "cannot execute browser process");
    } else {
        // Non-browser (renderer/gpu/utility) subprocess: execute_process
        // already ran the subprocess entry point above, nothing left to do.
        return ExitCode::from(0);
    }

    let settings = Settings {
        windowless_rendering_enabled: true as _,
        external_message_pump: true as _,
        ..Default::default()
    };
    assert_eq!(
        initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()),
        1,
        "CEF initialize failed"
    );

    let mut event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    let exit_code = loop {
        do_message_loop_work();
        match event_loop.pump_app_events(Some(Duration::ZERO), &mut app) {
            PumpStatus::Exit(code) => break ExitCode::from(code as u8),
            PumpStatus::Continue => {}
        }
        sleep(Duration::from_millis(1000 / 60));
    };

    cef::shutdown();
    exit_code
}
