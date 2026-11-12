// Release builds on Windows open no console window; logs still go to
// --log-file.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use shine::ShineApp;
use shine::logging::{self, LogConfig};

use shine::APP_ID;

/// Faster than the system allocator when simulating on every thread.
#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> eframe::Result {
    // Command-line use: no window, nothing but the output.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("simulate") => std::process::exit(shine::cli::main(&args[1..])),
        Some("export") => std::process::exit(shine::cli::export::main(&args[1..])),
        Some("layer") => std::process::exit(shine::cli::layer::main(&args[1..])),
        Some("--version" | "-V") => {
            println!("shine {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "Shine {} - Photonic Circuit Simulator\n\nUsage:\n  shine [PROJECT.shine [--doc NAME]] [--log-file PATH] [--log-level FILTER]\n                                                 open the editor, on a project (and one of its designs)\n  shine simulate PROJECT.shine [options]        simulate from the command line (--help for options)\n  shine export PROJECT.shine [options]          S-parameters or a gdsfactory layout (--help for options)\n  shine layer MATRIX --out PROJECT.shine        a matrix as a component of MZI meshes (--help for options)\n  shine --version",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        _ => {}
    }
    // A project to open, and which of its designs to show.
    let (mut open, mut doc) = (None, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--log-file" | "--log-level" => {
                rest.next();
            }
            "--doc" => doc = rest.next().cloned(),
            a if a.starts_with('-') => {}
            path => open = Some(std::path::PathBuf::from(path)),
        }
    }
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
            if let Some(path) = &open {
                match app.workspace.open_path(path) {
                    Ok(()) => {
                        let id = doc.as_deref().and_then(|name| app.workspace.project.find(name));
                        if let Some(id) = id {
                            app.workspace.open_tab(id);
                        }
                    }
                    Err(e) => app.workspace.notify(format!("Can't open {}: {e}", path.display())),
                }
            }
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = &result {
        tracing::error!("Shine exited with an error: {e}");
    }
    result
}
