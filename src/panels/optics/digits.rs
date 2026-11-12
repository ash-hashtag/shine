//! An optical neural network that reads handwritten digits: a small but
//! real AI inference chip, simulated end to end.
//!
//! The network (trained by `reference/digits/train.py` on the UCI
//! handwritten digits, CC BY 4.0, Alpaydin & Kaynak 1998) takes a digit as a
//! 4×4 thumbnail, 16 values in 0…1, and computes
//!
//! ```text
//! h = clamp(|W1 x|² / H, 0, 1)      layer 1: light, photodetectors, electronics
//! o = |W2 h|²                        layer 2: fresh light, detectors
//! digit = argmax(o)
//! ```
//!
//! On the chip:
//!
//! - **Inputs**: 16 value inputs laid out as the 4×4 image, each driving a
//!   linear modulator (a push-pull Mach–Zehnder whose driver electronics
//!   make its field exactly the value) on one of 16 lights split from a
//!   1550 nm laser.
//! - **Layer 1**: W1 (16×16) as two 16×16 MZI meshes with an attenuator per
//!   mode between them (W1 = U Σ V†, scaled so the largest Σ is 1).
//! - **Neurons**: 16 photodetectors with electronics (function cells) that
//!   turn each detected power into a drive, clamp(G·p, 0, 1): the network's
//!   nonlinearity, as in optoelectronic neural-network chips.
//! - **Layer 2**: 16 more modulators on the laser's other half, W2 (10×16)
//!   as a 16×16 mesh, 10 attenuators and a 10×10 mesh.
//! - **Readout**: electronics read the 10 output powers and show the
//!   largest's index, the digit, on a display.

use std::f64::consts::PI;

use serde::Deserialize;

use super::model::{BlockRef, Builtin, InstanceId, Item, Layout, OpticComponentKind, PlacedComponent, Terminal, cell_center};
use super::reference::{ARM_UM, LOSS_DB_PER_CM, WAVELENGTH_NM, add, arm, link, mesh_named};
use crate::panels::project::{DocId, Project};
use crate::photonics::{C64, Matrix, UnitaryMesh};

pub const INPUTS: usize = 16;
pub const HIDDEN: usize = 16;
pub const CLASSES: usize = 10;

/// The trained network.
#[derive(Clone, Debug, Deserialize)]
pub struct Weights {
    pub w1: Vec<Vec<f64>>,
    pub w2: Vec<Vec<f64>>,
    /// H: the hidden powers are divided by it (and clamped to 1).
    pub hidden_scale: f64,
    pub test_accuracy: f64,
}

/// Test digits: 4×4 thumbnails (row by row) and what each is.
#[derive(Clone, Debug, Deserialize)]
pub struct TestDigits {
    pub pixels: Vec<Vec<f64>>,
    pub labels: Vec<usize>,
}

pub fn weights() -> Weights {
    serde_json::from_str(include_str!("../../../reference/digits/weights.json")).expect("the bundled weights")
}

pub fn test_digits() -> TestDigits {
    serde_json::from_str(include_str!("../../../reference/digits/test-digits.json")).expect("the bundled test digits")
}

fn matrix(rows: &[Vec<f64>]) -> Matrix {
    Matrix::from_fn(rows.len(), rows[0].len(), |i, j| C64::new(rows[i][j], 0.0))
}

/// The network's output powers for `pixels`, computed digitally: what the
/// chip should detect, up to one overall factor.
pub fn scores(w: &Weights, pixels: &[f64]) -> Vec<f64> {
    let layer = |m: &[Vec<f64>], x: &[f64]| -> Vec<f64> { m.iter().map(|row| row.iter().zip(x).map(|(a, b)| a * b).sum::<f64>()).collect() };
    let h: Vec<f64> = layer(&w.w1, pixels).iter().map(|y| (y * y / w.hidden_scale).clamp(0.0, 1.0)).collect();
    layer(&w.w2, &h).iter().map(|y| y * y).collect()
}

pub fn argmax(v: &[f64]) -> usize {
    v.iter().enumerate().fold((0, f64::NEG_INFINITY), |b, (i, &x)| if x > b.1 { (i, x) } else { b }).0
}

/// One layer `m` (rows × cols) as meshes: V† (cols × cols), the per-mode
/// amplitudes (σ/σmax, one per row), and U (rows × rows), each mesh in the
/// rectangular arrangement; and σmax.
pub struct Svd {
    pub v_adjoint: UnitaryMesh,
    pub attenuation: Vec<f64>,
    pub u: UnitaryMesh,
    pub scale: f64,
}

pub fn svd(m: &Matrix) -> Result<Svd, String> {
    let (rows, cols) = (m.nrows(), m.ncols());
    if rows > cols {
        return Err("a layer can't have more outputs than inputs".into());
    }
    // Pad to square: the extra rows are zero, so their singular values are
    // zero and U is block diagonal; its rows × rows block is the mesh.
    let padded = Matrix::from_fn(cols, cols, |i, j| if i < rows { m[(i, j)] } else { C64::ZERO });
    let s = padded.svd().map_err(|_| "the SVD didn't converge".to_owned())?;
    let sigma: Vec<f64> = (0..rows).map(|i| s.S().column_vector()[i].re).collect();
    let scale = sigma.iter().copied().fold(0.0, f64::max);
    let u = Matrix::from_fn(rows, rows, |i, j| s.U()[(i, j)]);
    let v_adjoint = s.V().adjoint().to_owned();
    Ok(Svd {
        v_adjoint: UnitaryMesh::decompose_rectangular(&v_adjoint).map_err(|e| e.to_string())?,
        attenuation: sigma.iter().map(|s| s / scale).collect(),
        u: UnitaryMesh::decompose_rectangular(&u).map_err(|e| e.to_string())?,
        scale,
    })
}

pub const MODULATOR: &str = "Linear modulator";

/// A push-pull Mach–Zehnder modulator with its driver: the field out is the
/// drive value (−1…1) times the field in, with no phase change. The driver
/// sets the arms to ±(π/2 − asin x), so they add up to cos(π/2 − asin x) = x.
pub fn linear_modulator(project: &mut Project) -> DocId {
    if let Some(id) = project.find(MODULATOR).filter(|&id| project.doc(id).is_some_and(|d| d.component.is_some())) {
        return id;
    }
    let mut l = Layout::default();
    let input = add(&mut l, Builtin::InputPin { name: "in".into() }, (0, 0));
    let split = add(&mut l, Builtin::YSplitter, (3, 0));
    let upper_heater = add(&mut l, Builtin::PhaseShifter { phase: PI / 2.0 }, (6, -2));
    let lower_heater = add(&mut l, Builtin::PhaseShifter { phase: -PI / 2.0 }, (6, 2));
    let upper = add(&mut l, arm(), (9, -2));
    let lower = add(&mut l, arm(), (9, 2));
    let join = add(&mut l, Builtin::YCombiner, (12, 0));
    let output = add(&mut l, Builtin::OutputPin { name: "out".into() }, (15, 0));
    link(&mut l, (input, 0), (split, 0));
    link(&mut l, (split, 0), (upper_heater, 0));
    link(&mut l, (split, 1), (lower_heater, 0));
    link(&mut l, (upper_heater, 0), (upper, 0));
    link(&mut l, (lower_heater, 0), (lower, 0));
    link(&mut l, (upper, 0), (join, 0));
    link(&mut l, (lower, 0), (join, 1));
    link(&mut l, (join, 0), (output, 0));
    // The driver: the value in, the two heaters' drives out (a heater adds
    // its drive × π).
    let x = "asin(clamp(in1, -1, 1))";
    let driver = add(
        &mut l,
        Builtin::FunctionCell {
            name: "driver".into(),
            inputs: 1,
            light_inputs: 0,
            outputs: vec![format!("-{x} / pi"), format!("{x} / pi")],
            light_outputs: Vec::new(),
            wavelength_nm: WAVELENGTH_NM,
        },
        (5, -6),
    );
    l.components.push(PlacedComponent { kind: OpticComponentKind::SignalIn, pos: cell_center(5, -9) });
    let pin = l.components.len() - 1;
    l.set_label(pin, "value");
    let signal = |l: &mut Layout, from: (Item, usize), to: (Item, usize)| l.connect_signal(Terminal { item: from.0, port: from.1 }, Terminal { item: to.0, port: to.1 });
    signal(&mut l, (Item::Part(pin), 0), (Item::Box(driver), 0));
    signal(&mut l, (Item::Box(driver), 0), (Item::Box(upper_heater), 0));
    signal(&mut l, (Item::Box(driver), 1), (Item::Box(lower_heater), 0));
    project.add_component(
        MODULATOR,
        "LMOD",
        "A push-pull Mach–Zehnder modulator with its driver: the field it passes is its value (−1 to 1) times the field in, with no change of phase. The driver electronics set the two arms' heaters to ±(π/2 − asin value), so the arms add up to exactly the value.",
        l,
        10.0,
    )
}

/// A layer `m` as a component: V† mesh, an attenuator per output, U mesh;
/// inputs past the outputs end in absorbers.
fn layer(project: &mut Project, name: &str, m: &Matrix) -> Result<(DocId, f64, usize), String> {
    let s = svd(m)?;
    let (rows, cols) = (m.nrows(), m.ncols());
    let v = mesh_named(project, &s.v_adjoint, &format!("{name}: V† ({cols}×{cols} mesh)"), "V†");
    let u = mesh_named(project, &s.u, &format!("{name}: U ({rows}×{rows} mesh)"), "U");
    let mut l = Layout::default();
    let v_box = l.add_instance(BlockRef::Component(v), (3, 0));
    let u_box = l.add_instance(BlockRef::Component(u), (12, 0));
    for k in 0..cols {
        let pin = add(&mut l, Builtin::InputPin { name: format!("in{}", k + 1) }, (0, k as i32));
        link(&mut l, (pin, 0), (v_box, k));
        if k < rows {
            let loss_db = -20.0 * s.attenuation[k].max(1e-12).log10();
            let a = add(&mut l, Builtin::Attenuator { loss_db }, (8, k as i32));
            link(&mut l, (v_box, k), (a, 0));
            link(&mut l, (a, 0), (u_box, k));
            let out = add(&mut l, Builtin::OutputPin { name: format!("out{}", k + 1) }, (17, k as i32));
            link(&mut l, (u_box, k), (out, 0));
        } else {
            // A mode the layer doesn't use: absorbed.
            let dump = add(&mut l, Builtin::Detector, (8, k as i32));
            link(&mut l, (v_box, k), (dump, 0));
        }
    }
    let id = project.add_component(
        name,
        "W",
        &format!(
            "A {rows}×{cols} weight matrix in light: a {cols}×{cols} MZI mesh (V†), an attenuator per output (the singular values, the largest passing everything) and a {rows}×{rows} mesh (U). It computes W x / {:.4}, W's largest singular value.",
            s.scale
        ),
        l,
        10.0,
    );
    Ok((id, s.scale, s.v_adjoint.depth() + s.u.depth()))
}

/// Field amplitude through one arm (or one mesh column).
fn arm_amplitude() -> f64 {
    10f64.powf(-LOSS_DB_PER_CM * ARM_UM * 1e-4 / 20.0)
}

/// What the chip is built from, for checking it.
pub struct Chip {
    pub top: DocId,
    /// The layer 2 box: its outputs are the 10 scores.
    pub layer2: InstanceId,
    /// The 16 pixel inputs, row by row.
    pub pixels: Vec<InstanceId>,
    /// Detected power per unit of |W2 h|²: the scores the chip reads are
    /// [`scores`] times this.
    pub score_scale: f64,
    /// The field entering a layer's input per unit of its modulator's value
    /// (laser, splitters and the modulator's arm).
    pub input_field: f64,
    /// The same for layer 2's inputs (its own splitter tree).
    pub input_field2: f64,
    /// The neurons' gain: drive = clamp(gain × detected power, 0, 1).
    pub neuron_gain: f64,
}

/// The digit-reading chip for `w`, showing `pixels` (16 values, row by row).
pub fn chip(project: &mut Project, w: &Weights, pixels: &[f64]) -> Result<Chip, String> {
    chip_named(project, "Digit recognizer (optical neural network)", w, pixels)
}

impl Weights {
    /// Weights as `reference/mnist/train.py` and `reference/digits/train.py` write them.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let w: Self = serde_json::from_str(text).map_err(|e| format!("can't read the weights: {e}"))?;
        let inputs = w.w1.first().map_or(0, Vec::len);
        if inputs == 0 || w.w1.iter().any(|r| r.len() != inputs) || w.w2.iter().any(|r| r.len() != w.w1.len()) {
            return Err("the weights' shapes don't fit together (W1 hidden × inputs, W2 classes × hidden)".into());
        }
        Ok(w)
    }

    /// Inputs, hidden neurons and classes.
    pub fn sizes(&self) -> (usize, usize, usize) {
        (self.w1.first().map_or(0, Vec::len), self.w1.len(), self.w2.len())
    }
}

/// A splitter tree for `n` outputs: the levels, and the field each output
/// gets of 1 (a tree's outputs are a power of two; any beyond `n` go unused).
fn tree_for(n: usize) -> (u32, f64) {
    let levels = n.max(1).next_power_of_two().trailing_zeros();
    (levels, 1.0 / ((1usize << levels) as f64).sqrt())
}

/// The chip for `w` of any size (its inputs a square image, row by row),
/// named `name`, showing `pixels`.
pub fn chip_named(project: &mut Project, name: &str, w: &Weights, pixels: &[f64]) -> Result<Chip, String> {
    let (inputs, hidden, classes) = w.sizes();
    let modulator = linear_modulator(project);
    let (layer1, scale1, columns1) = layer(project, &format!("Layer 1 ({inputs}→{hidden})"), &matrix(&w.w1))?;
    let (layer2, scale2, columns2) = layer(project, &format!("Layer 2 ({hidden}→{classes})"), &matrix(&w.w2))?;
    let (levels1, split1) = tree_for(inputs);
    let (levels2, split2) = tree_for(hidden);
    // Field reaching each modulator: the laser's 1 mW halved, then split by each tree.
    let feed = 1.0 / 2f64.sqrt() * split1;
    let feed2 = 1.0 / 2f64.sqrt() * split2;
    let t = arm_amplitude();
    // Layer 1's detected power is |W1 x|² × this; the neurons undo it and divide by H.
    let gain1 = (feed * t * t.powi(columns1 as i32) / scale1).powi(2);
    let neuron_gain = 1.0 / (gain1 * w.hidden_scale);
    let score_scale = (feed2 * t * t.powi(columns2 as i32) / scale2).powi(2);

    let mut l = Layout::default();
    // The image: a value input per pixel in a square grid, top left.
    let side = (inputs as f64).sqrt().ceil() as usize;
    let mut pixel_boxes = Vec::new();
    for k in 0..inputs {
        let (r, c) = ((k / side) as i32, (k % side) as i32);
        let value = pixels.get(k).copied().unwrap_or(0.0);
        pixel_boxes.push(add(&mut l, Builtin::Value { name: String::new(), value }, (8 * c, 2 * r)));
    }
    // Columns left to right: the image, the laser and trees, modulators,
    // layer 1, neurons, layer 2's modulators, layer 2, the readout.
    let x = 8 * side as i32 + 24;
    let laser = add(&mut l, Builtin::Laser { amplitude: 1.0, phase: 0.0, wavelength_nm: WAVELENGTH_NM }, (x - 22, 2 * side as i32 + 4));
    let halves = add(&mut l, Builtin::YSplitter, (x - 17, 2 * side as i32 + 4));
    link(&mut l, (laser, 0), (halves, 0));
    let tree1 = add(&mut l, Builtin::SplitterTree { levels: levels1 }, (x - 8, 12));
    let tree2 = add(&mut l, Builtin::SplitterTree { levels: levels2 }, (x + 18, 8));
    link(&mut l, (halves, 0), (tree1, 0));
    link(&mut l, (halves, 1), (tree2, 0));
    let l1 = l.add_instance(BlockRef::Component(layer1), (x + 6, 8));
    let l2 = l.add_instance(BlockRef::Component(layer2), (x + 30, 8));
    let signal = |l: &mut Layout, from: (InstanceId, usize), to: (InstanceId, usize)| {
        l.connect_signal(Terminal { item: Item::Box(from.0), port: from.1 }, Terminal { item: Item::Box(to.0), port: to.1 })
    };
    for (k, &pixel) in pixel_boxes.iter().enumerate() {
        // Layer 1's modulators, driven by the pixels.
        let m1 = l.add_instance(BlockRef::Component(modulator), (x, 2 * k as i32));
        signal(&mut l, (pixel, 0), (m1, 0));
        link(&mut l, (tree1, k), (m1, 0));
        link(&mut l, (m1, 0), (l1, k));
    }
    for k in 0..hidden {
        let row = 2 * k as i32;
        // A neuron: a photodetector and its electronics.
        let neuron = add(
            &mut l,
            Builtin::FunctionCell {
                name: format!("n{}", k + 1),
                inputs: 0,
                light_inputs: 1,
                outputs: vec![format!("clamp(p1 * {neuron_gain:.9e}, 0, 1)")],
                light_outputs: Vec::new(),
                wavelength_nm: WAVELENGTH_NM,
            },
            (x + 12, row),
        );
        link(&mut l, (l1, k), (neuron, 0));
        // Layer 2's modulators, driven by the neurons, on the laser's other half.
        let m2 = l.add_instance(BlockRef::Component(modulator), (x + 24, row));
        signal(&mut l, (neuron, 0), (m2, 0));
        link(&mut l, (tree2, k), (m2, 0));
        link(&mut l, (m2, 0), (l2, k));
    }
    // The readout: the largest of the 10 scores, as a number.
    let args: Vec<String> = (1..=classes).map(|k| format!("p{k}")).collect();
    let bits = (classes.max(2) - 1).ilog2() + 1;
    let readout = add(
        &mut l,
        Builtin::FunctionCell {
            name: "argmax".into(),
            inputs: 0,
            light_inputs: classes as u32,
            outputs: (0..bits).rev().map(|b| format!("bit(argmax({}), {b})", args.join(", "))).collect(),
            light_outputs: Vec::new(),
            wavelength_nm: WAVELENGTH_NM,
        },
        (x + 36, 11),
    );
    for k in 0..classes {
        link(&mut l, (l2, k), (readout, k));
    }
    let display = add(&mut l, Builtin::Display { bits }, (x + 36, 13 + classes as i32));
    for b in 0..bits as usize {
        signal(&mut l, (readout, b), (display, b));
    }
    let top = project.add_layout(name, l, 10.0);
    Ok(Chip { top, layer2: l2, pixels: pixel_boxes, score_scale, input_field: feed * t, input_field2: feed2 * t, neuron_gain })
}

/// The demo: the chip with the first test digit.
pub fn demo(project: &mut Project) -> DocId {
    let digits = test_digits();
    chip(project, &weights(), &digits.pixels[0]).expect("the bundled weights make a chip").top
}
