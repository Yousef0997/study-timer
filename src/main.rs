//! Study Timer: a daily study scheduler with spoken alarms for Linux desktops.

mod alerts;
mod autostart;
mod model;
#[cfg(feature = "snapshot")]
mod snapshot;
mod theme;
mod ui;

use chrono::{Local, Timelike};
use eframe::egui::{self, ViewportCommand};
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("study-timer.sock"),
        None => std::env::temp_dir().join(format!("study-timer-{}.sock", std::env::var("USER").unwrap_or_default())),
    }
}

fn main() -> eframe::Result {
    let background = std::env::args().any(|a| a == "--background");

    // Only one copy may run, otherwise every alarm would ring twice. A second launch just raises the first.
    let socket = socket_path();
    if let Ok(mut stream) = UnixStream::connect(&socket) {
        let _ = stream.write_all(b"show\n");
        println!("Study Timer is already running; bringing it to the front.");
        return Ok(());
    }
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).ok();

    let data = Arc::new(Mutex::new(model::Data::load()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Study Timer")
            .with_app_id("study-timer")
            .with_inner_size([1040.0, 700.0])
            .with_min_inner_size([520.0, 480.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "study-timer",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            spawn_scheduler(data.clone(), cc.egui_ctx.clone());
            if let Some(listener) = listener {
                spawn_raise_listener(listener, cc.egui_ctx.clone());
            }
            Ok(Box::new(ui::StudyApp::new(data, background)))
        }),
    );
    let _ = std::fs::remove_file(&socket);
    result
}

/// Checks the schedule every second, independent of whether the window is visible.
fn spawn_scheduler(data: Arc<Mutex<model::Data>>, ctx: egui::Context) {
    thread::spawn(move || loop {
        let (fired, sound, voice) = {
            let mut d = ui::lock(&data);
            let (fired, changed) = d.tick(Local::now());
            if changed {
                if let Err(e) = d.save() {
                    eprintln!("study-timer: could not save: {e}");
                }
            }
            (fired, d.settings.sound, d.settings.voice)
        };
        if !fired.is_empty() {
            ctx.request_repaint();
        }
        alerts::fire(fired, sound, voice);
        let ms_into_second = Local::now().nanosecond() / 1_000_000;
        thread::sleep(Duration::from_millis(1000 - ms_into_second.min(999) as u64));
    });
}

fn spawn_raise_listener(listener: UnixListener, ctx: egui::Context) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            if stream.is_ok() {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
                ctx.request_repaint();
            }
        }
    });
}
