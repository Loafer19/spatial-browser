// Compositor: owns the window, the GPU surface, and the spatial canvas.
// Multiple CEF pages, each off-screen rendered (CPU path — see
// browser.rs) into its own textured quad, placed side by side in canvas
// space. See camera.rs for the world<->screen mapping (pan/zoom) and
// app.rs for the hit-testing/z-order/drag logic that makes it a canvas
// rather than just two fixed windows.
//
// CEF's multi-process model means this same binary is re-exec'd as the
// renderer/gpu/utility helper processes, so the CEF bootstrap (execute_process
// / initialize) has to run at the very top of main(), before any window or
// wgpu setup, and the winit loop has to cooperatively pump CEF's message
// loop (do_message_loop_work) instead of blocking in `run_app`.

mod app;
mod bookmarks;
mod browser;
mod camera;
mod hotkeys;
mod input;
mod output;
mod persistence;
mod session;
mod single_instance;

use app::App;
use cef::{args::Args, *};
use cef_bridge::{AppBuilder, OsrApp};
use std::{process::ExitCode, thread::sleep, time::Duration};
use winit::{
    event_loop::{ControlFlow, EventLoop},
    platform::pump_events::{EventLoopExtPumpEvents, PumpStatus},
};

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

    // Checked only for the actual browser process, never a re-exec'd
    // subprocess above (which would otherwise collide with the running
    // instance's own lock and abort CEF entirely). A conflict here means
    // we're done before paying for CEF's initialize() below.
    if single_instance::acquire_or_notify() {
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
