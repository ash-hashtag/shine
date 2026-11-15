//! How simulation time grows with the size of a design: 1 to 32 copies of
//! the Full adder component (lasers, interferometers, detectors, electronics),
//! side by side; then ripple-carry adders of 8 to 64 of them, each one's
//! carry into the next, simulated, and simulated again after one input bit
//! changes (what an edit costs).
//!
//! `cargo run --release --example scale_bench`

use std::time::Instant;

use shine::panels::circuit::sim::{Machine, Options, simulate};
use shine::panels::optics::model::{BlockRef, Builtin, InstanceId, Item, Layout, Terminal};
use shine::panels::project::Project;

fn main() {
    // Results go through the logger (stderr; --log-file to keep them).
    let _log = shine::logging::init(&shine::logging::LogConfig::from_env()).expect("logging");
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
        tracing::info!("{n:>3} full adders: {ms:>8.2} ms per simulation ({} sources){growth}", first.wavelengths.len());
        previous = Some(ms);
    }

    for n in [8, 16, 32, 64] {
        let mut p = Project::new();
        let adder = shine::panels::optics::examples::import_component(&mut p, 0).expect("the Full adder example");
        let mut layout = Layout::default();
        let value = |l: &mut Layout, v: f64, cell| l.add_instance(BlockRef::Builtin(Builtin::Value { name: String::new(), value: v }), cell);
        let mut carry: Option<InstanceId> = None;
        let mut first_input = None;
        for k in 0..n {
            let x = 6 * k;
            let a = value(&mut layout, (k % 3 == 0) as u8 as f64, (x, 0));
            let b = value(&mut layout, (k % 2 == 0) as u8 as f64, (x + 2, 0));
            first_input.get_or_insert(a);
            let fa = layout.add_instance(BlockRef::Component(adder), (x, 3));
            let wire = |l: &mut Layout, from: InstanceId, port, to: InstanceId, to_port| {
                l.connect_signal(Terminal { item: Item::Box(from), port }, Terminal { item: Item::Box(to), port: to_port })
            };
            wire(&mut layout, a, 0, fa, 0);
            wire(&mut layout, b, 0, fa, 1);
            if let Some(c) = carry {
                wire(&mut layout, c, 1, fa, 2);
            }
            carry = Some(fa);
        }
        let run = |l: &Layout| simulate(l, 10.0, &p, &Machine::default(), None, &Options::default());
        let start = Instant::now();
        let first = run(&layout);
        let first_ms = start.elapsed().as_secs_f64() * 1e3;
        assert!(first.error.is_none(), "{:?}", first.error);
        // Flip the first bit, as an edit does, and simulate again.
        let reps = 5;
        let start = Instant::now();
        for r in 0..reps {
            let id = first_input.expect("an input");
            if let Some(BlockRef::Builtin(Builtin::Value { value, .. })) = layout.instances.iter_mut().find(|i| i.id == id).map(|i| &mut i.block) {
                *value = (r % 2) as f64;
            }
            std::hint::black_box(run(&layout));
        }
        let edit_ms = start.elapsed().as_secs_f64() * 1e3 / reps as f64;
        tracing::info!("{n:>3}-bit ripple-carry adder: {first_ms:>8.1} ms the first time, {edit_ms:>8.1} ms after an input changes ({} rounds)", first.iterations);
    }
}
