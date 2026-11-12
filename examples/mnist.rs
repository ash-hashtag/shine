//! MNIST at full size, as an optical chip: 28×28 pixels into a 784-mode
//! MZI mesh. Shine's stress test: how long each step takes and how much
//! memory it needs, at a size no example has been before.
//!
//! First train the network (MNIST is downloaded into CACHE, not kept here):
//!
//!     pip install numpy jax
//!     python reference/mnist/train.py CACHE
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
//! - `--editor` times editor frames on the chip and inside its biggest mesh.

use std::time::Instant;

use shine::panels::circuit::sim::{self, Machine, Options};
use shine::panels::optics::digits::{self, Weights};
use shine::panels::optics::model::{BlockRef, Builtin};
use shine::panels::project::{DocId, Project};

/// Peak memory of this process so far, in MB (Linux).
fn peak_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok()))
        .map_or(f64::NAN, |kb| kb / 1024.0)
}

fn step<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = f();
    tracing::info!("{what}: {:.2} s, peak memory {:.0} MB", started.elapsed().as_secs_f64(), peak_mb());
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
    let cache = args.first().filter(|a| !a.starts_with("--")).expect("usage: mnist CACHE [--images N] [--save FILE] [--editor]").clone();
    let images: usize = value("--images").and_then(|v| v.parse().ok()).unwrap_or(5);

    let w = Weights::from_json(&std::fs::read_to_string(format!("{cache}/weights.json")).expect("CACHE/weights.json: run reference/mnist/train.py")).unwrap();
    let raw = std::fs::read(format!("{cache}/test-images.f32")).expect("test images");
    let labels = std::fs::read(format!("{cache}/test-labels.u8")).expect("test labels");
    let (inputs, hidden, classes) = w.sizes();
    let test: Vec<Vec<f64>> = raw.chunks_exact(4 * inputs).map(|c| c.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64).collect()).collect();
    tracing::info!("Network {inputs} → {hidden} → {classes}, trained to {:.2}% on {} test digits", 100.0 * w.test_accuracy, test.len());

    let mut p = Project::new();
    let chip = step("Build the chip (decompose the meshes, place every part)", || {
        digits::chip_named(&mut p, "MNIST digit recognizer", &w, &test[0]).expect("the chip")
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
        let o = step(&format!("Simulate the whole chip, digit {}", k + 1), || {
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
        tracing::info!("  it's a {}; the chip reads {read} ({} rounds, scores within {err:.1e})", labels[k], o.iterations);
    }
    if images > 0 {
        tracing::info!("  the chip and the network agree on {agree}/{} digits", images.min(test.len()));
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
    let (correct, same) = step("All test digits through the layers' matrices", || {
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
        "  {correct}/{} right ({:.2}%; the trained network {:.2}%); the same digit as the network on {same}",
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
    use egui::{Pos2, Rect, Vec2};
    let mut ws = shine::panels::workspace::Workspace::default();
    step("Open in the editor", || ws.open_bytes("mnist.shine", bytes).unwrap());
    let ctx = egui::Context::default();
    let mut t = 0.0;
    let mut frame = |ws: &mut shine::panels::workspace::Workspace, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1000.0))), time: Some(t), events, ..Default::default() };
        let started = Instant::now();
        ctx.run_ui(input, |ui| ws.ui(ui)).textures_delta.clear();
        started.elapsed().as_secs_f64() * 1e3
    };
    let first = frame(&mut ws, Vec::new());
    let idle: Vec<f64> = (0..10).map(|_| frame(&mut ws, Vec::new())).collect();
    tracing::info!("Editor on the chip: first frame {first:.0} ms, then {:.1} ms a frame", idle.iter().sum::<f64>() / 10.0);
    let biggest = ws.project.documents.iter().max_by_key(|d| d.layout().instances.len()).unwrap().id;
    ws.open_tab(biggest);
    let first = frame(&mut ws, Vec::new());
    let idle: Vec<f64> = (0..10).map(|_| frame(&mut ws, Vec::new())).collect();
    tracing::info!("Editor inside the biggest mesh: first frame {first:.0} ms, then {:.1} ms a frame, peak memory {:.0} MB", idle.iter().sum::<f64>() / 10.0, peak_mb());
}

#[cfg(not(feature = "gui"))]
fn editor(_: &[u8]) {
    tracing::warn!("--editor needs the app (the gui feature)");
}
