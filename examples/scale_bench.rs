//! How simulation time grows with the size of a design: 1 to 32 copies of
//! the Full adder component (lasers, interferometers, detectors, electronics).
//!
//! `cargo run --release --example scale_bench`

use std::time::Instant;

use shine::panels::circuit::sim::{Machine, Options, simulate};
use shine::panels::optics::model::{BlockRef, Layout};
use shine::panels::project::Project;

fn main() {
    let mut previous: Option<f64> = None;
    for n in [1, 2, 4, 8, 16, 32] {
        let mut p = Project::new();
        let adder = shine::panels::optics::examples::import_component(&mut p, 0).expect("the Full adder example");
        let mut layout = Layout::default();
        for k in 0..n {
            layout.add_instance(BlockRef::Component(adder), (0, 8 * k));
        }
        let run = || simulate(&layout, 10.0, &p, &Machine::default(), None, &Options::default());
        let first = run();
        assert!(first.error.is_none(), "{:?}", first.error);
        let reps = 10;
        let start = Instant::now();
        for _ in 0..reps {
            std::hint::black_box(run());
        }
        let ms = start.elapsed().as_secs_f64() * 1e3 / reps as f64;
        let growth = previous.map_or_else(String::new, |p| format!("  ×{:.1} for twice the size", ms / p));
        println!("{n:>3} full adders: {ms:>8.2} ms per simulation ({} sources){growth}", first.wavelengths.len());
        previous = Some(ms);
    }
}
