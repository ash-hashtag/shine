//! The reference design: a programmable photonic matrix processor, the core
//! of optical neural-network and signal-processing chips, at a realistic
//! size. It is how we know what Shine can and can't do with a large design:
//! tests check it simulates correctly and stays quick to open, draw and
//! edit, and `cargo run --release --example reference_design` measures it.
//!
//! - **MZI cell**: two 50:50 directional couplers, two heaters (θ between
//!   the arms, φ on the top input) and two 100 µm silicon arms.
//! - **N×N mesh**: N(N−1)/2 MZI cells in the rectangular (Clements)
//!   arrangement, N columns deep, with a matching waveguide wherever a mode
//!   skips a column (so every path has the same length and loss) and an
//!   output phase on each mode. It is programmed to a unitary matrix.
//! - **Modulator**: a Mach–Zehnder modulator (Y-splitter, a heater, two arms,
//!   Y-combiner) whose drive signal v (0 to 1) sets the field it passes,
//!   sin(πv/2): none at 0, all at 1, half the power at 0.5.
//! - **Processor**: a 1550 nm laser, a 1×N splitter tree, a modulator per
//!   input driven by a value input above it (the input vector), the mesh,
//!   and a detector per output showing its reading: the detectors read
//!   |U x|².
//!
//! As shipped, the mesh is programmed to the N-point discrete Fourier
//! transform and the inputs to a square wave with period 4 (1 1 0 0 …), so
//! the detectors light up at frequency bins 0, N/4 and 3N/4. Set the inputs
//! (click along them) and the laser (select it) and watch the spectrum.

use std::f64::consts::PI;

use super::model::{BlockRef, Builtin, Export, InstanceId, Item, Layout, OpticComponentKind, PlacedComponent, PortRef, Terminal, cell_center};
use crate::panels::project::{DocId, Project};
use crate::photonics::{C64, Matrix, UnitaryMesh};

/// The size of the shipped design.
pub const SIZE: usize = 16;
/// The length of each MZI arm, and of the waveguide where a mode skips a column.
pub const ARM_UM: f64 = 100.0;
/// Silicon strip waveguide at 1550 nm.
pub const N_EFF: f64 = 2.4;
pub const N_G: f64 = 4.2;
pub const LOSS_DB_PER_CM: f64 = 2.0;
pub const WAVELENGTH_NM: f64 = 1550.0;

pub const MZI_CELL: &str = "MZI cell";
pub const MODULATOR: &str = "Modulator";

fn arm() -> Builtin {
    Builtin::DelayLine { length_um: ARM_UM, n_eff: N_EFF, n_g: N_G, reference_nm: WAVELENGTH_NM, loss_db_per_cm: LOSS_DB_PER_CM }
}

fn link(l: &mut Layout, from: (InstanceId, usize), to: (InstanceId, usize)) {
    l.connect_optical(PortRef { instance: from.0, port: from.1 }, PortRef { instance: to.0, port: to.1 });
}

fn add(l: &mut Layout, b: Builtin, cell: (i32, i32)) -> InstanceId {
    l.add_instance(BlockRef::Builtin(b), cell)
}

/// The MZI cell: `Mzi(θ, φ)` built from its parts, with `θ` and `φ`
/// exported for each box to set.
pub fn mzi_cell(project: &mut Project) -> DocId {
    if let Some(id) = project.find(MZI_CELL).filter(|&id| project.doc(id).is_some_and(|d| d.component.is_some())) {
        return id;
    }
    let mut l = Layout::default();
    let a = add(&mut l, Builtin::InputPin { name: "a".into() }, (0, 0));
    let b = add(&mut l, Builtin::InputPin { name: "b".into() }, (0, 1));
    let phi = add(&mut l, Builtin::PhaseShifter { phase: 0.0 }, (3, 0));
    let split = add(&mut l, Builtin::Coupler { coupling: 0.5 }, (6, 0));
    let theta = add(&mut l, Builtin::PhaseShifter { phase: 0.0 }, (9, 0));
    let upper = add(&mut l, arm(), (12, 0));
    let lower = add(&mut l, arm(), (12, 1));
    let join = add(&mut l, Builtin::Coupler { coupling: 0.5 }, (15, 0));
    let out_a = add(&mut l, Builtin::OutputPin { name: "a".into() }, (18, 0));
    let out_b = add(&mut l, Builtin::OutputPin { name: "b".into() }, (18, 1));
    link(&mut l, (a, 0), (phi, 0));
    link(&mut l, (phi, 0), (split, 0));
    link(&mut l, (b, 0), (split, 1));
    link(&mut l, (split, 0), (theta, 0));
    link(&mut l, (theta, 0), (upper, 0));
    link(&mut l, (split, 1), (lower, 0));
    link(&mut l, (upper, 0), (join, 0));
    link(&mut l, (lower, 0), (join, 1));
    link(&mut l, (join, 0), (out_a, 0));
    link(&mut l, (join, 1), (out_b, 0));
    l.exports.push(Export { name: "θ".into(), item: Item::Box(theta), param: "phase".into() });
    l.exports.push(Export { name: "φ".into(), item: Item::Box(phi), param: "phase".into() });
    project.add_component(
        MZI_CELL,
        "MZI",
        "A thermally tuned Mach–Zehnder interferometer: two 50:50 couplers, a heater θ between 100 µm arms that sets the split (θ = 0 crosses, π is the bar state) and a heater φ on the top input that sets the phase. The unit cell of programmable photonic meshes.",
        l,
        10.0,
    )
}

/// The N×N mesh programmed to `mesh`, as a component (named for its size).
pub fn mesh(project: &mut Project, mesh: &UnitaryMesh) -> DocId {
    let cell = mzi_cell(project);
    let n = mesh.size;
    let mut l = Layout::default();
    const PITCH: i32 = 5;
    let column_x = |c: usize| 3 + PITCH * c as i32;
    // The open end of each mode: (box, port).
    let mut front: Vec<(InstanceId, usize)> = (0..n).map(|m| (add(&mut l, Builtin::InputPin { name: format!("in{}", m + 1) }, (0, m as i32)), 0)).collect();
    let columns = mesh.columns();
    let depth = mesh.depth();
    for c in 0..depth {
        let mut busy = vec![false; n];
        for (s, _) in mesh.mzis.iter().zip(&columns).filter(|(_, col)| **col == c) {
            let m = s.mode;
            let b = l.add_instance(BlockRef::Component(cell), (column_x(c), m as i32));
            if let Some(inst) = l.instances.iter_mut().find(|i| i.id == b) {
                inst.set_param("θ", Some(s.theta));
                inst.set_param("φ", Some(s.phi));
            }
            for k in 0..2 {
                link(&mut l, front[m + k], (b, k));
                front[m + k] = (b, k);
                busy[m + k] = true;
            }
        }
        // A mode skipping this column goes through the same length of waveguide.
        for m in (0..n).filter(|&m| !busy[m]) {
            let w = add(&mut l, arm(), (column_x(c), m as i32));
            link(&mut l, front[m], (w, 0));
            front[m] = (w, 0);
        }
    }
    let x = column_x(depth);
    for (m, &p) in mesh.output_phases.iter().enumerate() {
        let ps = add(&mut l, Builtin::PhaseShifter { phase: p }, (x, m as i32));
        link(&mut l, front[m], (ps, 0));
        let out = add(&mut l, Builtin::OutputPin { name: format!("out{}", m + 1) }, (x + 3, m as i32));
        link(&mut l, (ps, 0), (out, 0));
    }
    project.add_component(
        &format!("{n}×{n} mesh"),
        &format!("U{n}"),
        &format!(
            "{} MZI cells in the rectangular (Clements) arrangement, {depth} columns deep, programmed to a {n}×{n} unitary matrix; every path crosses the same length of waveguide, so losses are balanced.",
            mesh.mzis.len()
        ),
        l,
        10.0,
    )
}

/// The N-point discrete Fourier transform, `U[j][k] = e^{−2πi jk/N} / √N`.
pub fn dft(n: usize) -> Matrix {
    Matrix::from_fn(n, n, |j, k| C64::from_polar(1.0 / (n as f64).sqrt(), -2.0 * PI * (j * k) as f64 / n as f64))
}

/// The modulator: light in, Y-splitter, a heater on the top arm, two arms,
/// Y-combiner, light out. Its drive signal v adds vπ to the heater's phase θ
/// (π unless set otherwise): at 0 the arms cancel, at 1 they add up; the
/// field out is sin(πv/2) of the field in.
pub fn modulator(project: &mut Project) -> DocId {
    if let Some(id) = project.find(MODULATOR).filter(|&id| project.doc(id).is_some_and(|d| d.component.is_some())) {
        return id;
    }
    let mut l = Layout::default();
    let input = add(&mut l, Builtin::InputPin { name: "in".into() }, (0, 0));
    let split = add(&mut l, Builtin::YSplitter, (3, 0));
    let heater = add(&mut l, Builtin::PhaseShifter { phase: PI }, (6, -1));
    let upper = add(&mut l, arm(), (9, -1));
    let lower = add(&mut l, arm(), (9, 1));
    let join = add(&mut l, Builtin::YCombiner, (12, 0));
    let output = add(&mut l, Builtin::OutputPin { name: "out".into() }, (15, 0));
    link(&mut l, (input, 0), (split, 0));
    link(&mut l, (split, 0), (heater, 0));
    link(&mut l, (heater, 0), (upper, 0));
    link(&mut l, (split, 1), (lower, 0));
    link(&mut l, (upper, 0), (join, 0));
    link(&mut l, (lower, 0), (join, 1));
    link(&mut l, (join, 0), (output, 0));
    // The control: a signal pin above the heater.
    l.components.push(PlacedComponent { kind: OpticComponentKind::SignalIn, pos: cell_center(6, -4) });
    let on = l.components.len() - 1;
    l.set_label(on, "drive");
    l.connect_signal(Terminal { item: Item::Part(on), port: 0 }, Terminal { item: Item::Box(heater), port: 0 });
    l.exports.push(Export { name: "θ".into(), item: Item::Box(heater), param: "phase".into() });
    project.add_component(
        MODULATOR,
        "MOD",
        "A thermo-optic Mach–Zehnder modulator: the light is split between two 100 µm arms and joined again. With the heater at θ = π (as it starts) the arms cancel and nothing comes out; a drive signal v adds vπ, so it passes sin²(πv/2) of the power: none at 0, half at 0.5, all at 1.",
        l,
        10.0,
    )
}

/// The whole processor: laser, splitter tree, a modulator per input driven
/// by a value input (`inputs`, 0 to 1: the input vector), the mesh programmed
/// to `u`, and detectors. Returns the top document.
pub fn processor(project: &mut Project, u: &Matrix, inputs: &[f64]) -> Result<DocId, String> {
    let n = u.nrows();
    if !n.is_power_of_two() || n < 2 {
        return Err(format!("the splitter tree needs a power of two inputs, not {n}"));
    }
    let programmed = UnitaryMesh::decompose_rectangular(u).map_err(|e| e.to_string())?;
    let modulator = modulator(project);
    let mesh_doc = mesh(project, &programmed);
    let mut l = Layout::default();
    let rows = 2 * n as i32;
    let laser = add(&mut l, Builtin::Laser { amplitude: 1.0, phase: 0.0, wavelength_nm: WAVELENGTH_NM }, (0, rows / 2 - 1));
    let tree = add(&mut l, Builtin::SplitterTree { levels: n.trailing_zeros() }, (4, rows / 2 - n as i32 / 2));
    link(&mut l, (laser, 0), (tree, 0));
    let mesh_box = l.add_instance(BlockRef::Component(mesh_doc), (18, rows / 2 - n as i32 / 2));
    for k in 0..n {
        let row = 2 * k as i32;
        let m = l.add_instance(BlockRef::Component(modulator), (11, row));
        // Its input, just above it: click along it to set it.
        let input = add(&mut l, Builtin::Value { value: inputs.get(k).copied().unwrap_or(0.0) }, (11, row - 1));
        l.connect_signal(Terminal { item: Item::Box(input), port: 0 }, Terminal { item: Item::Box(m), port: 0 });
        link(&mut l, (tree, k), (m, 0));
        link(&mut l, (m, 0), (mesh_box, k));
        let d = add(&mut l, Builtin::Detector, (24, 2 * k as i32));
        link(&mut l, (mesh_box, k), (d, 0));
    }
    Ok(project.add_layout(&format!("{n}×{n} photonic matrix processor"), l, 10.0))
}

/// The field a modulator passes for drive `v` (apart from its arms' common
/// loss and phase): `(e^{iπ(1+v)} + 1)/2 = −i e^{iπv/2} sin(πv/2)`. Its size
/// is sin(πv/2); its phase moves with v too, as with any modulator driven on
/// one arm.
pub fn modulator_field(v: f64) -> C64 {
    (C64::from_polar(1.0, PI * (1.0 + v)) + 1.0) / 2.0
}

/// The shipped input: a square wave with period 4 (1 1 0 0 …).
pub fn input_values(n: usize) -> Vec<f64> {
    (0..n).map(|k| if k % 4 < 2 { 1.0 } else { 0.0 }).collect()
}

/// The shipped design, in a new project: the 16-point DFT of a square wave.
pub fn reference_project() -> Project {
    sized_project(SIZE)
}

/// The design at another size (a power of two), for finding Shine's limits.
pub fn sized_project(n: usize) -> Project {
    let mut p = Project::new();
    processor(&mut p, &dft(n), &input_values(n)).expect("the DFT is unitary and n a power of two");
    // The processor first: it's what opens.
    p.documents.rotate_right(1);
    p
}

/// The demo: the reference design added to a project.
pub fn demo(project: &mut Project) -> DocId {
    processor(project, &dft(SIZE), &input_values(SIZE)).expect("the DFT is unitary and 16 a power of two")
}

/// The power each detector should read, from the matrices alone: the
/// laser's 1 mW split N ways, each modulator's field for its input v, the
/// unitary, and the waveguide loss of the N + 1 columns every path crosses.
pub fn expected_powers(u: &Matrix, inputs: &[f64]) -> Vec<f64> {
    let inputs: Vec<C64> = inputs.iter().map(|&v| modulator_field(v)).collect();
    let n = u.nrows();
    let split = 1.0 / (n as f64).sqrt();
    let loss = 10f64.powf(-LOSS_DB_PER_CM * ARM_UM * 1e-4 / 20.0).powi(n as i32 + 1);
    (0..n)
        .map(|j| {
            let field: C64 = (0..n).map(|k| u[(j, k)] * inputs.get(k).copied().unwrap_or_default() * split).sum();
            (field * loss).norm_sqr()
        })
        .collect()
}
