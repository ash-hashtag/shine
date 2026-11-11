//! The optical neural network that reads handwritten digits
//! (`shine::panels::optics::digits`): its parts do what they should, and
//! simulated as a chip, light, detectors and electronics together, it reads
//! digits as the trained network does.

use shine::panels::circuit::sim::{self, Machine, Options};
use shine::panels::optics::digits::{self, CLASSES};
use shine::panels::optics::model::{BlockRef, Builtin, Item, Layout, PortRef, Terminal};
use shine::panels::project::Project;
use shine::photonics::{C64, Matrix};

/// The saved chip, regenerated with `cargo run --example digits -- --write`.
const SAVED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/reference/digits/digit-recognizer.shine");

/// The chip's 10 scores (detected powers) for `pixels`, and the digit its display shows.
fn read(p: &mut Project, chip: &digits::Chip, pixels: &[f64]) -> (Vec<f64>, usize) {
    let layout = &mut p.doc_mut(chip.top).unwrap().panel_mut().layout;
    for (id, &v) in chip.pixels.iter().zip(pixels) {
        let inst = layout.instances.iter_mut().find(|i| i.id == *id).unwrap();
        inst.block = BlockRef::Builtin(Builtin::Value { name: String::new(), value: v });
    }
    let doc = p.doc(chip.top).unwrap();
    let o = sim::simulate(doc.layout(), 10.0, p, &Machine::default(), Some(chip.top), &Options::default());
    assert!(o.error.is_none() && o.converged, "{:?}", o.error);
    let scores = (0..CLASSES).map(|k| o.port_power[&(chip.layer2, k)]).collect();
    // The display's 4 bits, most significant first.
    let display = doc.layout().instances.iter().find(|i| matches!(i.block, BlockRef::Builtin(Builtin::Display { .. }))).unwrap().id;
    let shown = (0..4).fold(0, |n, b| (n << 1) | usize::from(o.input(doc.layout(), Terminal { item: Item::Box(display), port: b }) >= 0.5));
    (scores, shown)
}

#[test]
fn the_linear_modulator_passes_its_value_as_the_field() {
    // Drive it with values from −1 to 1: the field out is the value times
    // the field in (and its arm's loss and phase), signs and all.
    let mut p = Project::new();
    let m = digits::linear_modulator(&mut p);
    let mut fields = Vec::new();
    for v in [-1.0, -0.4, 0.0, 0.3, 1.0] {
        let mut l = Layout::default();
        let input = l.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "in".into() }), (0, 0));
        let value = l.add_instance(BlockRef::Builtin(Builtin::Value { name: String::new(), value: v }), (3, -3));
        let modulator = l.add_instance(BlockRef::Component(m), (3, 0));
        let out = l.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "out".into() }), (8, 0));
        l.connect_optical(PortRef { instance: input, port: 0 }, PortRef { instance: modulator, port: 0 });
        l.connect_optical(PortRef { instance: modulator, port: 0 }, PortRef { instance: out, port: 0 });
        l.connect_signal(Terminal { item: Item::Box(value), port: 0 }, Terminal { item: Item::Box(modulator), port: 0 });
        let id = p.add_layout("probe", l, 10.0);
        let d = p.doc(id).unwrap();
        let t = sim::transmission(d.layout(), 10.0, &p, &Machine::default(), Some(id), &[1550.0]).unwrap();
        fields.push((v, t.matrices[0][(0, 0)]));
    }
    // The same arm factor for every value.
    let arm = fields.iter().find(|f| f.0 == 1.0).unwrap().1;
    for (v, f) in fields {
        assert!((f - arm * v).norm() < 1e-12, "value {v}: {f} vs {}", arm * v);
    }
}

#[test]
fn a_layer_is_its_weight_matrix_over_its_largest_singular_value() {
    let w = Matrix::from_fn(3, 4, |i, j| C64::new(((i * 4 + j) as f64).sin(), 0.0));
    let s = digits::svd(&w).unwrap();
    assert_eq!(s.v_adjoint.size, 4);
    assert_eq!(s.u.size, 3, "an output mesh only as big as the outputs");
    let sigma = Matrix::from_fn(3, 4, |i, j| C64::new(if i == j { s.attenuation[i] } else { 0.0 }, 0.0));
    let rebuilt = s.u.matrix() * sigma * s.v_adjoint.matrix();
    for i in 0..3 {
        for j in 0..4 {
            assert!((rebuilt[(i, j)] * s.scale - w[(i, j)]).norm() < 1e-9);
        }
    }
    assert!(s.attenuation.iter().all(|a| (0.0..=1.0 + 1e-12).contains(a)), "passive: {:?}", s.attenuation);
}

#[test]
fn the_chip_reads_digits_as_the_network_does() {
    let w = digits::weights();
    let test = digits::test_digits();
    let mut p = Project::new();
    let chip = digits::chip(&mut p, &w, &test.pixels[0]).unwrap();
    for pixels in test.pixels.iter().take(6) {
        let (got, shown) = read(&mut p, &chip, pixels);
        let want: Vec<f64> = digits::scores(&w, pixels).iter().map(|s| s * chip.score_scale).collect();
        let top = want.iter().cloned().fold(0.0, f64::max);
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 1e-9 * top, "{got:?}\n{want:?}");
        }
        assert_eq!(shown, digits::argmax(&got), "the display shows the brightest output");
    }
}

#[test]
fn the_saved_chip_opens_and_reads_the_same() {
    let bytes = std::fs::read(SAVED).expect("reference/digits/digit-recognizer.shine");
    let mut saved = Project::from_bytes(&bytes).expect("it opens");
    let w = digits::weights();
    let test = digits::test_digits();
    let mut fresh = Project::new();
    let chip = digits::chip(&mut fresh, &w, &test.pixels[0]).unwrap();
    // The same chip: find it in the saved copy by its documents' names and sizes.
    let shape = |p: &Project| {
        let mut v: Vec<_> = p.documents.iter().map(|d| (d.name.clone(), d.layout().instances.len(), d.layout().wires.len())).collect();
        v.sort();
        v
    };
    assert_eq!(shape(&saved), shape(&fresh), "the generator changed: regenerate with `cargo run --example digits -- --write`");
    // Built the same way, its boxes have the same ids.
    let top = saved.find("Digit recognizer (optical neural network)").unwrap();
    let saved_chip = digits::Chip { top, ..chip };
    let digit = &test.pixels[1];
    let (got, shown) = read(&mut saved, &saved_chip, digit);
    let want: Vec<f64> = digits::scores(&w, digit).iter().map(|s| s * saved_chip.score_scale).collect();
    let top = want.iter().cloned().fold(0.0, f64::max);
    assert!(got.iter().zip(&want).all(|(g, w)| (g - w).abs() < 1e-9 * top), "{got:?}\n{want:?}");
    assert_eq!(shown, digits::argmax(&want));
}
