//! How the asset library copes with thousands of components: adding them,
//! saving, and loading at start.
//!
//! `cargo run --release --example library_bench [n]`

use std::time::Instant;

use shine::panels::assets::Library;
use shine::panels::optics::model::{Layout, OpticComponentKind, PlacedComponent, cell_center};
use shine::panels::project::Project;

fn main() {
    // Results go through the logger (stderr; --log-file to keep them).
    let _log = shine::logging::init(&shine::logging::LogConfig::from_env()).expect("logging");
    let n: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(5000);
    // One project with n components, imported at once.
    let mut p = Project::new();
    for k in 0..n {
        let layout = Layout::with_components(vec![PlacedComponent { kind: OpticComponentKind::OutputPort, pos: cell_center(0, 0) }]);
        p.add_component(&format!("Part {k:05}"), "P", &format!("Number {k}"), layout, 10.0);
    }
    let file = p.to_bytes();
    let start = Instant::now();
    let mut library = Library::new();
    library.import_file("parts.shine", &file, "Imported").unwrap();
    tracing::info!("import {n}:     {:>8.1} ms", start.elapsed().as_secs_f64() * 1e3);
    let start = Instant::now();
    let mut one = Project::new();
    let id = one.add_component("One more", "OM", "", Layout::default(), 10.0);
    library.add(&one.extract(id).unwrap(), "").unwrap();
    tracing::info!("add one more:   {:>8.2} ms", start.elapsed().as_secs_f64() * 1e3);
    let start = Instant::now();
    let bytes = library.to_bytes();
    tracing::info!("save:           {:>8.1} ms  ({} KB)", start.elapsed().as_secs_f64() * 1e3, bytes.len() / 1024);
    let start = Instant::now();
    let loaded = Library::from_bytes(&bytes).unwrap();
    tracing::info!("load:           {:>8.1} ms  ({} assets)", start.elapsed().as_secs_f64() * 1e3, loaded.len());
    let start = Instant::now();
    let hits = loaded.summaries().iter().filter(|a| a.matches("part 004")).count();
    tracing::info!("search:         {:>8.2} ms  ({hits} found)", start.elapsed().as_secs_f64() * 1e3);
}
