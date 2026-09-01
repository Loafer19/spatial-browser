// Window + wgpu + spatial canvas of CEF OSR pages. CEF re-execs this binary
// as helper processes — bootstrap before window/wgpu; pump do_message_loop_work.

mod app;
mod autofill_bridge;
mod browser;
mod clipboard_bridge;
mod file_dialog;
mod hotkeys;
mod hud;
mod input;
mod minimap;
mod output;
mod pages;
mod pending_actions;
mod persistence;
mod reader_mode;
mod session;
mod single_instance;
mod userscripts;
mod userstyles;
mod viewport;

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
    let ret = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );

    if is_browser_process {
        assert!(ret == -1, "cannot execute browser process");
    } else {
        // Non-browser (renderer/gpu/utility) subprocess: execute_process
        // already ran the subprocess entry point above, nothing left to do.
        return ExitCode::from(0);
    }

    // Browser process only — subprocesses would collide with the instance lock.
    if single_instance::acquire_or_notify() {
        return ExitCode::from(0);
    }

    // Own config dir (not CEF default): empty cache_path = incognito, wiped on restart.
    let home = std::env::var_os("HOME").expect("HOME not set");
    let cef_data_path: CefString = std::path::Path::new(&home)
        .join(".config/spatial-browser/cef_data")
        .to_string_lossy()
        .as_ref()
        .into();
    let settings = Settings {
        windowless_rendering_enabled: true as _,
        external_message_pump: true as _,
        cache_path: cef_data_path.clone(),
        root_cache_path: cef_data_path,
        ..Default::default()
    };
    assert_eq!(
        initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut()
        ),
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
        // Live Settings fps — don't hardcode 60 on top of CEF's own cap.
        sleep(Duration::from_millis(1000 / app.target_fps() as u64));
    };

    cef::shutdown();
    exit_code
}
