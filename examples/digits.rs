//! Runs the optical neural network that reads handwritten digits
//! (`shine::panels::optics::digits`) on all 450 test digits, simulating the
//! whole chip for each: light, detectors and electronics.
//!
//! `cargo run --release --example digits [-- --scores FILE] [-- --export DIR] [-- --write]`
//!
//! - `--scores FILE`: every digit's 10 detected scores and what it was, as JSON
//! - `--export DIR`: the two layers as SAX netlists and S-parameters, and the
//!   numbers that scale them, for running the network in other tools
//! - `--write`: save the chip to `reference/digits/digit-recognizer.shine`

use std::time::Instant;

use shine::panels::circuit::sim::{self, Machine, Options};
use shine::panels::interop::{self, sax};
use shine::panels::optics::digits::{self, CLASSES};
use shine::panels::optics::model::{BlockRef, Builtin};
use shine::panels::project::Project;

const SAVED: &str = "reference/digits/digit-recognizer.shine";

fn main() {
    let _log = shine::logging::init(&shine::logging::LogConfig::from_env()).expect("logging");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let w = digits::weights();
    let test = digits::test_digits();
    let mut p = Project::new();
    let started = Instant::now();
    let chip = digits::chip(&mut p, &w, &test.pixels[0]).expect("the chip");
    let built = started.elapsed();
    // The chip first: it's what opens.
    let at = p.documents.iter().position(|d| d.id == chip.top).unwrap();
    let top = p.documents.remove(at);
    p.documents.insert(0, top);
    let parts = {
        fn count(p: &Project, id: shine::panels::project::DocId) -> usize {
            p.doc(id).unwrap().layout().instances.iter().map(|i| match i.block {
                BlockRef::Component(d) => count(p, d),
                BlockRef::Builtin(_) => 1,
            }).sum()
        }
        count(&p, chip.top)
    };
    tracing::info!("Chip: {parts} parts when opened up, built in {built:?}");

    if args.iter().any(|a| a == "--write") {
        std::fs::write(SAVED, p.to_bytes()).expect("write the chip");
        tracing::info!("Wrote {SAVED}");
        return;
    }

    if let Some(dir) = value("--export") {
        std::fs::create_dir_all(&dir).expect("the export folder");
        let wl = [1550.0];
        // What the other tools do: each layer's matrix at 1550 nm.
        let started = Instant::now();
        for name in ["Layer 1 (16→16)", "Layer 2 (16→10)"] {
            let id = p.find(name).expect("the layer");
            let d = p.doc(id).unwrap();
            sim::transmission(d.layout(), 10.0, &p, &Machine::default(), Some(id), &wl).expect("the layer's matrix");
        }
        tracing::info!("Both layers' matrices at 1550 nm in {:?}", started.elapsed());
        for (name, file) in [("Layer 1 (16→16)", "layer1"), ("Layer 2 (16→10)", "layer2")] {
            let id = p.find(name).expect("the layer");
            let e = sax::export(&p, id, &wl, "digit-recognizer").expect("SAX export");
            std::fs::write(format!("{dir}/{file}.py"), &e.python).unwrap();
            let (ts, ports) = interop::touchstone(&p, id, &wl, "digit-recognizer").expect("S-parameters");
            std::fs::write(format!("{dir}/{file}.s{ports}p"), ts).unwrap();
            tracing::info!("{name}: {} instances, {} connections → {dir}/{file}.py", e.instances, e.connections);
        }
        // What the electronics around the layers do, for running the network elsewhere.
        let electronics = serde_json::json!({
            "input_field": chip.input_field,
            "neuron_gain": chip.neuron_gain,
            "score_scale": chip.score_scale,
        });
        std::fs::write(format!("{dir}/chip.json"), electronics.to_string()).unwrap();
    }

    // Every test digit through the chip.
    let doc_id = chip.top;
    let mut rows = Vec::new();
    let mut correct = 0;
    let mut agree = 0;
    let mut worst = 0.0f64;
    let started = Instant::now();
    for (pixels, &label) in test.pixels.iter().zip(&test.labels) {
        let layout = &mut p.doc_mut(doc_id).unwrap().panel_mut().layout;
        for (id, &v) in chip.pixels.iter().zip(pixels) {
            if let Some(inst) = layout.instances.iter_mut().find(|i| i.id == *id) {
                inst.block = BlockRef::Builtin(Builtin::Value { name: String::new(), value: v });
            }
        }
        let doc = p.doc(doc_id).unwrap();
        let o = sim::simulate(doc.layout(), 10.0, &p, &Machine::default(), Some(doc_id), &Options::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        let scores: Vec<f64> = (0..CLASSES).map(|k| o.port_power[&(chip.layer2, k)]).collect();
        let want: Vec<f64> = digits::scores(&w, pixels).iter().map(|s| s * chip.score_scale).collect();
        let top = want.iter().cloned().fold(0.0, f64::max);
        worst = worst.max(scores.iter().zip(&want).map(|(a, b)| (a - b).abs() / top).fold(0.0, f64::max));
        let read = digits::argmax(&scores);
        correct += usize::from(read == label);
        agree += usize::from(read == digits::argmax(&want));
        rows.push(serde_json::json!({ "label": label, "read": read, "scores": scores }));
    }
    let elapsed = started.elapsed();
    let n = test.labels.len();
    tracing::info!(
        "Read {n} digits in {elapsed:?} ({:.1} ms each): {correct} right ({:.1}%; the trained network: {:.1}%); agrees with the digital network on {agree}/{n}, scores within {worst:.1e} of the largest",
        elapsed.as_secs_f64() * 1e3 / n as f64,
        100.0 * correct as f64 / n as f64,
        100.0 * w.test_accuracy
    );
    if let Some(file) = value("--scores") {
        let out = serde_json::json!({ "score_scale": chip.score_scale, "seconds": elapsed.as_secs_f64(), "digits": rows });
        std::fs::write(&file, serde_json::to_string(&out).unwrap()).unwrap();
        tracing::info!("Scores → {file}");
    }
}
