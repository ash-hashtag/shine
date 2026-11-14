//! A 28×28-pixel classifier as an optical chip, trained on Fashion-MNIST
//! (the default) or MNIST: 784 pixels into a 784-mode MZI mesh. Shine's
//! stress test: how long each step takes and how much memory it needs, at a
//! size no example has been before.
//!
//! First train the network (the data is downloaded into CACHE, not kept here):
//!
//!     pip install numpy jax
//!     python reference/mnist/train.py CACHE [--dataset fashion|mnist]
//!
//! then
//!
//!     cargo run --release --example mnist -- CACHE [--images N] [--save FILE] [--editor]
//!
//! - simulates the whole chip (light, detectors, electronics) for N test
//!   digits (default 5) and checks each against the trained network;
//! - computes both layers' matrices once and runs all 10 000 test digits
//!   through them, as the chip would;
//! - `--save FILE` writes the chip, to open in the app;
//! - `--editor` times editor frames, idle and panning, on the chip, a small
//!   component and the biggest mesh; `--open FILE` (no CACHE) does only that,
//!   on a saved chip.
//!
//!
//! `--write-chip FILE` only builds the chip (its pixels dark) and saves it,
//! for `shine simulate FILE --input …` and `examples/mnist.py`, which read
//! digits of your own with it.

use std::time::Instant;

/// As the app: faster than the system allocator on every thread.
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use shine::panels::circuit::sim::{self, Machine, Options};
use shine::panels::optics::digits::{self, Weights};
use shine::panels::optics::model::{BlockRef, Builtin};
use shine::panels::project::{DocId, Project};

/// Memory of this process, in MB (Linux): `VmHWM` the peak so far, `VmRSS` now.
fn memory_mb(which: &str) -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with(which)).and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok()))
        .map_or(f64::NAN, |kb| kb / 1024.0)
}

fn peak_mb() -> f64 {
    memory_mb("VmHWM:")
}

fn step<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = profiled(what, f);
    tracing::info!("{what}: {:.2} s, peak memory {:.0} MB", started.elapsed().as_secs_f64(), peak_mb());
    out
}

/// A function's name without generic arguments: `a::B<C>::f` → `a::B::f`.
#[cfg(unix)]
fn short_name(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in name.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// With SHINE_PROFILE set to part of a step's name, samples that step (pprof,
/// in-process: no perf permissions needed) and logs where its time goes.
/// (Unix: pprof doesn't build on Windows, where steps just run.)
#[cfg(not(unix))]
fn profiled<T>(_what: &str, f: impl FnOnce() -> T) -> T {
    f()
}

/// With SHINE_PROFILE set to part of a step's name, samples that step (pprof,
/// in-process: no perf permissions needed) and logs where its time goes.
#[cfg(unix)]
fn profiled<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let wanted = std::env::var("SHINE_PROFILE").is_ok_and(|w| what.contains(&w));
    let guard = wanted.then(|| pprof::ProfilerGuardBuilder::default().frequency(999).build().expect("profiler"));
    let out = f();
    if let Some(report) = guard.and_then(|g| g.report().build().ok()) {
        // Samples in which each function is on the stack, and at its top.
        let mut inclusive: std::collections::HashMap<String, usize> = Default::default();
        let mut own: std::collections::HashMap<String, usize> = Default::default();
        let mut total = 0;
        for (frames, &count) in &report.data {
            let count = count.max(0) as usize;
            total += count;
            let names: Vec<String> = frames.frames.iter().flatten().map(|s| short_name(&s.name())).collect();
            if let Some(top) = names.first() {
                *own.entry(top.clone()).or_default() += count;
            }
            let mut seen = std::collections::HashSet::new();
            for n in names {
                if seen.insert(n.clone()) {
                    *inclusive.entry(n).or_default() += count;
                }
            }
        }
        for (title, map) in [("on the stack", inclusive), ("at the top", own)] {
            let runtime = ["std::", "core::", "alloc::", "rayon", "__", "call_once", "call_mut", "{closure", "mnist", "main", "fold", "for_each", "collect", "from_iter", "map", "extend", "spec_", "run_", "lang_start", "start_thread", "clone3", "backtrace", "pprof", "do_call", "try", "catch_unwind", "Iterator"];
            let mut rows: Vec<_> = map.into_iter().filter(|(n, _)| title == "at the top" || !runtime.iter().any(|r| n.starts_with(r))).collect();
            rows.sort_by_key(|r| std::cmp::Reverse(r.1));
            let lines: Vec<String> = rows.iter().take(40).map(|(n, c)| format!("{:5.1}% {n}", 100.0 * *c as f64 / total.max(1) as f64)).collect();
            tracing::info!("profile of “{what}”, {total} samples, {title}:\n{}", lines.join("\n"));
        }
    }
    out
}

fn parts(p: &Project, id: DocId) -> usize {
    p.doc(id).unwrap().layout().instances.iter().map(|i| match i.block {
        BlockRef::Component(d) => parts(p, d),
        BlockRef::Builtin(_) => 1,
    }).sum()
}

fn main() {
    let _log = shine::logging::init(&shine::logging::LogConfig::from_env()).expect("logging");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    // --open FILE: only the editor, on a saved chip.
    if let Some(file) = value("--open") {
        editor(&std::fs::read(&file).unwrap_or_else(|e| panic!("can't read {file}: {e}")));
        return;
    }
    let cache = args.first().filter(|a| !a.starts_with("--")).expect("usage: mnist CACHE [--images N] [--save FILE] [--editor] | CACHE --write-chip FILE").clone();
    let images: usize = value("--images").and_then(|v| v.parse().ok()).unwrap_or(5);

    let w = Weights::from_json(&std::fs::read_to_string(format!("{cache}/weights.json")).expect("CACHE/weights.json: run reference/mnist/train.py")).unwrap();
    if let Some(file) = value("--write-chip") {
        let mut p = Project::new();
        let blank = vec![0.0; w.sizes().0];
        step("Build the chip", || digits::chip_named(&mut p, &digits::chip_name(&w), &w, &blank).expect("the chip"));
        std::fs::write(&file, p.to_bytes()).unwrap_or_else(|e| panic!("can't write {file}: {e}"));
        tracing::info!("Wrote {file}: shine simulate {file} --input DIGIT.txt, or python examples/mnist.py --image DIGIT.png");
        return;
    }
    let raw = std::fs::read(format!("{cache}/test-images.f32")).expect("test images");
    let labels = std::fs::read(format!("{cache}/test-labels.u8")).expect("test labels");
    let (inputs, hidden, classes) = w.sizes();
    let test: Vec<Vec<f64>> = raw.chunks_exact(4 * inputs).map(|c| c.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b) as f64).collect()).collect();
    tracing::info!("Network {inputs} → {hidden} → {classes}, trained to {:.2}% on {} test images", 100.0 * w.test_accuracy, test.len());

    let mut p = Project::new();
    let chip = step("Build the chip (decompose the meshes, place every part)", || {
        digits::chip_named(&mut p, &digits::chip_name(&w), &w, &test[0]).expect("the chip")
    });
    let mzis = |d: &str| p.find(d).map_or(0, |id| p.doc(id).unwrap().layout().instances.len());
    tracing::info!(
        "  {} parts when opened up; the biggest mesh document has {} boxes and {} waveguides",
        parts(&p, chip.top),
        p.documents.iter().map(|d| d.layout().instances.len()).max().unwrap_or(0),
        p.documents.iter().map(|d| d.layout().wires.len()).max().unwrap_or(0),
    );
    let _ = mzis;
    let bytes = step("Save (to bytes)", || p.to_bytes());
    tracing::info!("  {:.1} MB", bytes.len() as f64 / 1e6);
    step("Open (from bytes)", || Project::from_bytes(&bytes).unwrap());
    if let Some(file) = value("--save") {
        std::fs::write(&file, &bytes).unwrap();
        tracing::info!("  saved to {file}");
    }

    // The whole chip, for a few digits.
    let mut agree = 0;
    for (k, pixels) in test.iter().take(images).enumerate() {
        let layout = &mut p.doc_mut(chip.top).unwrap().panel_mut().layout;
        for (id, &v) in chip.pixels.iter().zip(pixels) {
            if let Some(inst) = layout.instances.iter_mut().find(|i| i.id == *id) {
                inst.block = BlockRef::Builtin(Builtin::Value { name: String::new(), value: v });
            }
        }
        let doc = p.doc(chip.top).unwrap();
        let o = step(&format!("Simulate the whole chip, image {}", k + 1), || {
            sim::simulate(doc.layout(), 10.0, &p, &Machine::default(), Some(chip.top), &Options::default())
        });
        if let Some(e) = &o.error {
            tracing::error!("  {e}");
            break;
        }
        let got: Vec<f64> = (0..classes).map(|c| o.port_power.get(&(chip.layer2, c)).copied().unwrap_or(0.0)).collect();
        let want: Vec<f64> = digits::scores(&w, pixels).iter().map(|s| s * chip.score_scale).collect();
        let top = want.iter().cloned().fold(0.0, f64::max);
        let err = got.iter().zip(&want).map(|(a, b)| (a - b).abs() / top).fold(0.0, f64::max);
        let read = digits::argmax(&got);
        agree += usize::from(read == digits::argmax(&want));
        tracing::info!("  it's class {}; the chip reads {read} ({} rounds, scores within {err:.1e})", labels[k], o.iterations);
    }
    if images > 0 {
        tracing::info!("  the chip and the network agree on {agree}/{} images", images.min(test.len()));
    }

    // Both layers' matrices once, then every test digit through them.
    let layer_matrix = |name: &str| {
        let id = p.find(name).expect("the layer");
        let d = p.doc(id).unwrap();
        step(&format!("{name}: its matrix at 1550 nm"), || {
            sim::transmission(d.layout(), 10.0, &p, &Machine::default(), Some(id), &[1550.0]).expect("the layer's matrix").matrices.remove(0)
        })
    };
    let m1 = layer_matrix(&format!("Layer 1 ({inputs}→{hidden})"));
    let m2 = layer_matrix(&format!("Layer 2 ({hidden}→{classes})"));
    let (correct, same) = step("All test images through the layers' matrices", || {
        let mut correct = 0;
        let mut same = 0;
        for (pixels, &label) in test.iter().zip(&labels) {
            let y1: Vec<f64> = (0..hidden)
                .map(|j| (0..inputs).map(|k| m1[(j, k)] * (chip.input_field * pixels[k])).sum::<shine::photonics::C64>().norm_sqr())
                .collect();
            let h: Vec<f64> = y1.iter().map(|p| (chip.neuron_gain * p).clamp(0.0, 1.0)).collect();
            let o: Vec<f64> = (0..classes)
                .map(|c| (0..hidden).map(|j| m2[(c, j)] * (chip.input_field2 * h[j])).sum::<shine::photonics::C64>().norm_sqr())
                .collect();
            let read = digits::argmax(&o);
            correct += usize::from(read == label as usize);
            same += usize::from(read == digits::argmax(&digits::scores(&w, pixels)));
        }
        (correct, same)
    });
    tracing::info!(
        "  {correct}/{} right ({:.2}%; the trained network {:.2}%); the same answer as the network on {same}",
        test.len(),
        100.0 * correct as f64 / test.len() as f64,
        100.0 * w.test_accuracy
    );

    if args.iter().any(|a| a == "--editor") {
        editor(&bytes);
    }
}

#[cfg(feature = "gui")]
fn editor(bytes: &[u8]) {
    use egui::{Event, PointerButton, Pos2, Rect, Vec2};
    use shine::panels::workspace::Workspace;
    let mut ws = Workspace::default();
    let before = memory_mb("VmRSS:");
    step("Open in the editor", || ws.open_bytes("mnist.shine", bytes).unwrap());
    let ctx = egui::Context::default();
    let mut t = 0.0;
    let mut frame = |ws: &mut Workspace, events: Vec<Event>| {
        t += 1.0 / 60.0;
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1000.0))), time: Some(t), events, ..Default::default() };
        let started = Instant::now();
        ctx.run_ui(input, |ui| ws.ui(ui)).textures_delta.clear();
        started.elapsed().as_secs_f64() * 1e3
    };
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    // A view: its first frame, idle frames, then panning (a drag across the canvas).
    let mut view = |ws: &mut Workspace, name: &str| {
        let first = frame(ws, Vec::new());
        let idle: Vec<f64> = profiled(&format!("Idle frames: {name}"), || (0..10).map(|_| frame(ws, Vec::new())).collect());
        let at = |k: usize| Pos2::new(700.0 + 10.0 * k as f32, 500.0);
        frame(ws, vec![Event::PointerMoved(at(0)), Event::PointerButton { pos: at(0), button: PointerButton::Middle, pressed: true, modifiers: Default::default() }]);
        let pan: Vec<f64> = profiled(&format!("Panning: {name}"), || (1..=30).map(|k| frame(ws, vec![Event::PointerMoved(at(k))])).collect());
        frame(ws, vec![Event::PointerButton { pos: at(30), button: PointerButton::Middle, pressed: false, modifiers: Default::default() }]);
        tracing::info!(
            "Editor, {name}: first frame {first:.0} ms, idle {:.1} ms, panning {:.1} ms a frame (slowest {:.1}); the editor holds {:.0} MB",
            mean(&idle),
            mean(&pan),
            pan.iter().cloned().fold(0.0, f64::max),
            memory_mb("VmRSS:") - before
        );
    };
    let session = step("Encode the session (the app saves it every 5 s)", || ws.session().encode());
    tracing::info!("  {:.1} MB", session.len() as f64 / 1e6);
    view(&mut ws, "the chip");
    if let Some(m) = ws.project.find("Linear modulator") {
        ws.open_tab(m);
        view(&mut ws, "the linear modulator");
    }
    let biggest = ws.project.documents.iter().max_by_key(|d| d.layout().instances.len()).unwrap().id;
    ws.open_tab(biggest);
    view(&mut ws, "the biggest mesh");
}

#[cfg(not(feature = "gui"))]
fn editor(_: &[u8]) {
    tracing::warn!("--editor needs the app (the gui feature)");
}
