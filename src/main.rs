// Release builds on Windows open no console window; logs still go to
// --log-file.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use shine::ShineApp;
use shine::logging::{self, LogConfig};

fn main() -> eframe::Result {
    let _log_guard = match logging::init(&LogConfig::from_env()) {
        Ok(guard) => Some(guard),
        Err(e) => {
            eprintln!("shine: logging disabled: {e}");
            None
        }
    };
    tracing::info!("Starting Shine {}", env!("CARGO_PKG_VERSION"));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Shine",
        options,
        Box::new(|cc| Ok(Box::new(ShineApp::new(cc)))),
    );
    if let Err(e) = &result {
        tracing::error!("Shine exited with an error: {e}");
    }
    result
}
