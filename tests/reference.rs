//! The reference design (a programmable photonic matrix processor, see
//! `shine::panels::optics::reference`): it simulates to what its matrices
//! say, at the shipped size, and the copy saved in the repository still
//! opens and works as Shine's file format moves on.

use shine::panels::circuit::sim::{self, Machine, Options};
use shine::panels::optics::model::{BlockRef, Builtin, InstanceId};
use shine::panels::optics::reference::{self, SIZE};
use shine::panels::project::{DocId, Project};
use shine::photonics::{C64, Matrix};

/// The saved copy, regenerated with `cargo run --example reference_design -- --write`.
const SAVED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/reference/photonic-matrix-processor.shine");

fn top(p: &Project) -> DocId {
    p.documents.iter().find(|d| d.component.is_none()).expect("the processor").id
}

/// The mesh box of the processor, and the power leaving each of its outputs.
fn detected(p: &Project) -> Vec<f64> {
    let id = top(p);
    let doc = p.doc(id).unwrap();
    let o = sim::simulate(doc.layout(), doc.panel().cell_um, p, &Machine::default(), Some(id), &Options::default());
    assert!(o.error.is_none() && o.converged, "{:?}", o.error);
    let mesh = mesh_box(p);
    let n = (0..).take_while(|k| o.port_power.contains_key(&(mesh, *k))).count();
    (0..n).map(|k| o.port_power[&(mesh, k)]).collect()
}

fn mesh_box(p: &Project) -> InstanceId {
    let doc = p.doc(top(p)).unwrap();
    doc.layout()
        .instances
        .iter()
        .find(|i| matches!(i.block, BlockRef::Component(d) if p.doc(d).is_some_and(|d| d.name.contains("mesh"))))
        .expect("the mesh")
        .id
}

fn assert_powers(got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len());
    for (j, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((g - w).abs() < 1e-9, "output {j}: {g} mW, expected {w} mW");
    }
}

#[test]
fn a_small_processor_multiplies_by_a_random_unitary() {
    // 4×4, a unitary with no special structure.
    let a = Matrix::from_fn(4, 4, |r, c| C64::new(((r * 7 + c * 3) as f64).sin(), ((r * 5 + c * 11) as f64).cos()));
    let u = a.svd().unwrap().U().to_owned();
    let x = [true, false, true, true];
    let mut p = Project::new();
    reference::processor(&mut p, &u, &x).unwrap();
    assert_powers(&detected(&p), &reference::expected_powers(&u, &x));
}

#[test]
fn the_reference_design_is_an_optical_fourier_transform() {
    let p = reference::reference_project();
    let got = detected(&p);
    let want = reference::expected_powers(&reference::dft(SIZE), &reference::input_bits(SIZE));
    assert_powers(&got, &want);
    // A square wave with period 4: bins 0, N/4 and 3N/4 light up (its even
    // harmonic, N/2, cancels), the rest stay dark.
    for (j, w) in got.iter().enumerate() {
        let bright = [0, SIZE / 4, 3 * SIZE / 4].contains(&j);
        assert_eq!(*w > 1e-6, bright, "bin {j}: {w} mW");
    }
}

#[test]
fn clicking_an_input_bit_changes_the_spectrum() {
    // One bit on: an impulse, whose transform is flat: every bin the same.
    let mut p = reference::reference_project();
    let id = top(&p);
    let layout = &mut p.doc_mut(id).unwrap().panel_mut().layout;
    for inst in &mut layout.instances {
        if let BlockRef::Builtin(Builtin::NumberInput { value, .. }) = &mut inst.block {
            *value = 0;
        }
    }
    let first = layout.instances.iter_mut().find(|i| matches!(i.block, BlockRef::Builtin(Builtin::NumberInput { .. }))).unwrap();
    first.block = BlockRef::Builtin(Builtin::NumberInput { bits: 1, value: 1 });
    let mut impulse = vec![false; SIZE];
    impulse[0] = true;
    let got = detected(&p);
    assert_powers(&got, &reference::expected_powers(&reference::dft(SIZE), &impulse));
    assert!(got.iter().all(|w| (w - got[0]).abs() < 1e-12 && *w > 0.0), "flat: {got:?}");
}

#[test]
fn the_mesh_on_its_own_is_the_programmed_unitary() {
    // Its S-parameters at 1550 nm: the DFT, times the waveguides' common
    // loss and phase.
    let p = reference::reference_project();
    let mesh = p.documents.iter().find(|d| d.name.contains("mesh")).unwrap();
    let t = sim::transmission(mesh.layout(), 10.0, &p, &Machine::default(), Some(mesh.id), &[reference::WAVELENGTH_NM]).unwrap();
    let m = &t.matrices[0];
    let u = reference::dft(SIZE);
    let common = m[(0, 0)] / u[(0, 0)];
    let loss = 10f64.powf(-reference::LOSS_DB_PER_CM * reference::ARM_UM * 1e-4 / 20.0).powi(SIZE as i32);
    assert!((common.norm() - loss).abs() < 1e-12, "{} vs {loss}", common.norm());
    for j in 0..SIZE {
        for k in 0..SIZE {
            assert!((m[(j, k)] - common * u[(j, k)]).norm() < 1e-9, "U[{j}][{k}]");
        }
    }
}

#[test]
fn the_saved_copy_opens_and_works() {
    let bytes = std::fs::read(SAVED).expect("reference/photonic-matrix-processor.shine");
    let saved = Project::from_bytes(&bytes).expect("it opens");
    let fresh = reference::reference_project();
    assert_eq!(
        saved.documents.iter().map(|d| (&d.name, d.layout().instances.len(), d.layout().wires.len())).collect::<Vec<_>>(),
        fresh.documents.iter().map(|d| (&d.name, d.layout().instances.len(), d.layout().wires.len())).collect::<Vec<_>>(),
        "the generator changed: regenerate with `cargo run --example reference_design -- --write`"
    );
    assert_powers(&detected(&saved), &detected(&fresh));
}
