// Release builds on Windows open no console window; logs still go to
// --log-file.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use shine::ShineApp;
use shine::logging::{self, LogConfig};

/// The app's name for the OS: its settings folder and window class.
const APP_ID: &str = "Shine";

fn main() -> eframe::Result {
    let _log_guard = match logging::init(&LogConfig::from_env()) {
        Ok(guard) => Some(guard),
        Err(e) => {
            eprintln!("shine: logging disabled: {e}");
            None
        }
    };
    tracing::info!("Starting Shine {}", env!("CARGO_PKG_VERSION"));
    // Crash reports go next to the app's settings; one left by the last run
    // is shown at start.
    let last_crash = eframe::storage_dir(APP_ID).and_then(|dir| {
        let last = shine::crash::take_last(&dir);
        shine::crash::install(dir);
        last
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| {
            let mut app = ShineApp::new(cc);
            app.workspace.crash_report = last_crash;
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = &result {
        tracing::error!("Shine exited with an error: {e}");
    }
    result
}
