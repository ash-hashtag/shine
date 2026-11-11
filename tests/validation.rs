//! The validation page (docs/validation.md): each test here is one of its
//! rows, Shine's result against a closed-form, textbook one. They run with
//! every build, so the page stays true.

use std::f64::consts::PI;

use shine::panels::circuit::sim::{Machine, NoDocuments};
use shine::panels::circuit::sweep::{self, SweepKind, SweepSpec};
use shine::panels::circuit::transfer;
use shine::panels::optics::examples;
use shine::panels::optics::model::{InstanceId, OpticComponentKind};
use shine::photonics::{Block, C64, Circuit, Component, DirectionalCoupler, Mzi, Waveguide, blocks};

const C: f64 = 299_792_458.0;

/// Field from `input` to each output of a block, at `wl` nm.
fn fields(c: &Circuit, wl: f64, input: shine::photonics::InPort, outputs: &[shine::photonics::OutPort]) -> Vec<C64> {
    let t = c.transfer_matrix(wl, &[input], outputs).unwrap();
    (0..outputs.len()).map(|o| t[(o, 0)]).collect()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1e-300) || (a - b).abs() < 1e-12
}

#[test]
fn directional_coupler_splits_power_and_shifts_the_cross_port_by_90_degrees() {
    for k in [0.0, 0.1, 0.3, 0.5, 0.9, 1.0] {
        let s = DirectionalCoupler::new(k).s_matrix(1550.0);
        assert!(close(s[(0, 0)].norm_sqr(), 1.0 - k, 1e-12), "bar 1 − κ at κ = {k}");
        assert!(close(s[(1, 0)].norm_sqr(), k, 1e-12), "cross κ at κ = {k}");
        if k > 0.0 && k < 1.0 {
            let shift = (s[(1, 0)] / s[(0, 0)]).arg();
            assert!((shift - PI / 2.0).abs() < 1e-12, "cross leads by π/2, got {shift}");
        }
    }
}

#[test]
fn mzi_switch_sends_sin_squared_half_theta_through_the_bar() {
    for k in 0..=16 {
        let theta = 2.0 * PI * k as f64 / 16.0;
        let s = Mzi::new(theta, 0.0).s_matrix(1550.0);
        let bar = (theta / 2.0).sin().powi(2);
        assert!(close(s[(0, 0)].norm_sqr(), bar, 1e-12), "θ = {theta}");
        assert!((s[(0, 0)].norm_sqr() + s[(1, 0)].norm_sqr() - 1.0).abs() < 1e-12, "lossless");
    }
}

#[test]
fn unbalanced_mzi_fringes_are_lambda_squared_over_n_g_delta_l_apart() {
    // Two 50:50 couplers, arms 100 µm and 300 µm (n_g 4.2): FSR = λ²/(n_g ΔL).
    let mut c = Circuit::new();
    let a = c.add(DirectionalCoupler::new(0.5));
    let up = c.add(Waveguide::new(300.0, 2.4).with_group_index(4.2, 1550.0));
    let down = c.add(Waveguide::new(100.0, 2.4).with_group_index(4.2, 1550.0));
    let b = c.add(DirectionalCoupler::new(0.5));
    c.connect(a.output(0), up.input(0)).unwrap();
    c.connect(a.output(1), down.input(0)).unwrap();
    c.connect(up.output(0), b.input(0)).unwrap();
    c.connect(down.output(0), b.input(1)).unwrap();
    let xs: Vec<f64> = (0..20001).map(|k| 1530.0 + 40.0 * k as f64 / 20000.0).collect();
    let ys: Vec<f64> = xs.iter().map(|&wl| fields(&c, wl, a.input(0), &[b.output(0)])[0].norm_sqr()).collect();
    let s = sweep::spectrum(&xs, &ys).expect("fringes");
    let expected = 1550.0_f64.powi(2) / (4.2 * 200.0 * 1000.0);
    assert!(close(s.fsr_nm.unwrap(), expected, 0.01), "FSR {:?} vs {expected}", s.fsr_nm);
}

#[test]
fn all_pass_ring_resonance_depth_spacing_width_and_q() {
    let (kappa, length, loss, n_g) = (0.05, 100.0, 3.0, 4.2);
    let mut c = Circuit::new();
    let ring = c.add_block(&blocks::AllPassRing { coupling: kappa, length_um: length, n_eff: 2.4, loss_db_per_cm: loss, n_g, reference_nm: 1550.0 }).unwrap();
    let xs: Vec<f64> = (0..40001).map(|k| 1540.0 + 20.0 * k as f64 / 40000.0).collect();
    let ys: Vec<f64> = xs.iter().map(|&wl| fields(&c, wl, ring.inputs[0], &[ring.outputs[0]])[0].norm_sqr()).collect();
    let s = sweep::spectrum(&xs, &ys).expect("resonances");
    let r = (1.0 - kappa).sqrt();
    let a = 10f64.powf(-loss * length * 1e-4 / 20.0);
    let fsr = 1550.0_f64.powi(2) / (n_g * length * 1000.0);
    let fwhm = (1.0 - r * a) * 1550.0_f64.powi(2) / (PI * n_g * length * 1000.0 * (r * a).sqrt());
    let depth = (r - a).powi(2) / (1.0 - r * a).powi(2);
    assert!(close(s.fsr_nm.unwrap(), fsr, 0.01), "FSR = λ²/(n_g L)");
    assert!(close(s.fwhm_nm.unwrap(), fwhm, 0.05), "width {} vs {fwhm}", s.fwhm_nm.unwrap());
    assert!(close(s.q.unwrap(), 1550.0 / fwhm, 0.05), "Q");
    let min = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(close(min, depth, 0.02), "dip (r − a)²/(1 − ra)²: {min} vs {depth}");
}

#[test]
fn add_drop_ring_through_and_drop_at_resonance() {
    // Unequal couplings and loss; no dispersion, so the resonance is at
    // m λ / n_eff exactly.
    let (k1, k2, loss, n_eff) = (0.1, 0.05, 20.0, 2.4);
    let m = 200.0;
    let length = m * 1.55 / n_eff;
    let ring = blocks::AddDropRing { coupling_in: k1, coupling_drop: k2, length_um: length, n_eff, loss_db_per_cm: loss, n_g: n_eff, reference_nm: 1550.0 };
    let mut c = Circuit::new();
    let p = c.add_block(&ring).unwrap();
    let out = fields(&c, 1550.0, p.inputs[0], &p.outputs);
    let (r1, r2) = ((1.0 - k1).sqrt(), (1.0 - k2).sqrt());
    let a = 10f64.powf(-loss * length * 1e-4 / 20.0);
    let through = (r2 * a - r1).powi(2) / (1.0 - r1 * r2 * a).powi(2);
    let drop = k1 * k2 * a / (1.0 - r1 * r2 * a).powi(2);
    assert!(close(out[0].norm_sqr(), through, 1e-6), "through {} vs {through}", out[0].norm_sqr());
    assert!(close(out[1].norm_sqr(), drop, 1e-6), "drop {} vs {drop}", out[1].norm_sqr());
}

#[test]
fn waveguide_delay_is_n_g_l_over_c_and_loss_is_in_db() {
    let mut c = Circuit::new();
    let wg = c.add(Waveguide::new(2000.0, 2.4).with_group_index(4.2, 1550.0).with_loss(3.0));
    let xs: Vec<f64> = (0..101).map(|k| 1549.0 + 0.02 * k as f64).collect();
    let t: Vec<C64> = xs.iter().map(|&wl| fields(&c, wl, wg.input(0), &[wg.output(0)])[0]).collect();
    let p = transfer::path(&xs, &t);
    let delay = 2e-3 * 4.2 / C * 1e12;
    assert!(p.group_delay_ps.iter().all(|&d| close(d, delay, 1e-6)), "{} ps", delay);
    // 3 dB/cm over 0.2 cm: 0.6 dB.
    assert!(close(-10.0 * p.power[50].log10(), 0.6, 1e-9), "{}", -10.0 * p.power[50].log10());
}

#[test]
fn fabry_perot_bench_peaks_dips_spacing_and_finesse() {
    // The example: two mirrors of R = 0.9 a cavity apart, in free space.
    let l = examples::fabry_perot();
    let laser = l.components.iter().position(|c| matches!(c.kind, OpticComponentKind::Laser { .. })).unwrap();
    let spec = SweepSpec { kind: SweepKind::Wavelength { laser }, start: 600.0, stop: 700.0, points: 20001 };
    let names = |id: InstanceId| format!("{}", id.0);
    let r = sweep::run(&l, examples::FABRY_PEROT_CELL_UM, &NoDocuments, &Machine::default(), None, &spec, &names);
    assert!(r.error.is_none(), "{:?}", r.error);
    let det = r.series.iter().find(|s| !s.from_box).expect("the detector");
    let refl: f64 = 0.9;
    let (max, min) = det.power.iter().fold((0.0f64, f64::INFINITY), |(a, b), &p| (a.max(p), b.min(p)));
    assert!(close(max, 1.0, 1e-3), "peak transmission 1: {max}");
    assert!(close(min, ((1.0 - refl) / (1.0 + refl)).powi(2), 0.01), "valley ((1−R)/(1+R))²: {min}");
    let s = sweep::spectrum(&r.xs, &det.power).expect("resonances");
    let finesse = PI * refl.sqrt() / (1.0 - refl);
    let finesse_measured = s.fsr_nm.unwrap() / s.fwhm_nm.unwrap();
    assert!(close(finesse_measured, finesse, 0.03), "finesse π√R/(1−R): {finesse_measured} vs {finesse}");
    // Spacing λ²/(2 L): the mirrors are 2 cells apart.
    let cavity_nm = 2.0 * examples::FABRY_PEROT_CELL_UM * 1000.0;
    let centre = s.resonances[s.resonances.len() / 2];
    assert!(close(s.fsr_nm.unwrap(), centre * centre / (2.0 * cavity_nm), 0.03), "FSR λ²/(2L)");
}

#[test]
fn optical_dft_is_unitary_with_equal_power_to_every_output() {
    for n in [2usize, 4, 8, 16] {
        let mut c = Circuit::new();
        let ports = blocks::Dft { n }.build(&mut c).unwrap();
        let t = c.transfer_matrix(1550.0, &ports.inputs, &ports.outputs).unwrap();
        for i in 0..n {
            for o in 0..n {
                assert!(close(t[(o, i)].norm_sqr(), 1.0 / n as f64, 1e-9), "N = {n}: |U|² = 1/N");
            }
            let column: f64 = (0..n).map(|o| t[(o, i)].norm_sqr()).sum();
            assert!(close(column, 1.0, 1e-9), "unitary");
        }
    }
}

#[test]
fn lossless_lattice_conserves_power_over_all_wavelengths() {
    // 32 cascaded unequal-arm MZIs: whatever the interference, nothing is lost.
    let mut c = Circuit::new();
    let first = c.add(DirectionalCoupler::new(0.5));
    let mut prev = first;
    for _ in 0..32 {
        let up = c.add(Waveguide::new(150.0, 2.4).with_group_index(4.2, 1550.0));
        let down = c.add(Waveguide::new(100.0, 2.4).with_group_index(4.2, 1550.0));
        let next = c.add(DirectionalCoupler::new(0.5));
        c.connect(prev.output(0), up.input(0)).unwrap();
        c.connect(prev.output(1), down.input(0)).unwrap();
        c.connect(up.output(0), next.input(0)).unwrap();
        c.connect(down.output(0), next.input(1)).unwrap();
        prev = next;
    }
    for k in 0..2001 {
        let wl = 1500.0 + 0.05 * k as f64;
        let out = fields(&c, wl, first.input(0), &[prev.output(0), prev.output(1)]);
        assert!(close(out[0].norm_sqr() + out[1].norm_sqr(), 1.0, 1e-9), "at {wl} nm");
    }
}
