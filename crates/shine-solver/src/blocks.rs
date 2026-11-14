//! Library of reusable photonic-chip building blocks.
//!
//! Each block implements [`Block`], so it can be dropped into any
//! [`Circuit`] and wired to other blocks:
//!
//! | block | use on a chip |
//! |---|---|
//! | [`SplitterTree`] / [`CombinerTree`] | 1xN fan-out / N:1 fan-in |
//! | [`AllPassRing`] | notch filter, modulator, delay line |
//! | [`AddDropRing`] | channel drop filter |
//! | [`UnbalancedMzi`] | interleaver, FIR filter stage |
//! | [`SwitchTree`] | 1xN optical router addressed by bits |
//! | [`Dft`] | optical Fourier transform |
//! | [`WdmDemux`] | wavelength-division demultiplexer |
//! | [`SvdMesh`](super::SvdMesh) | matrix-vector multiplier |

use std::f64::consts::PI;

use super::circuit::{Block, Circuit, CircuitError, InPort, NodeId, OutPort, Ports};
use super::mesh::UnitaryMesh;
use super::{
    C64, DirectionalCoupler, Matrix, Mzi, PhaseShifter, Waveguide, YCombiner, YSplitter,
    propagation_phase,
};

/// A zero-length wire, used where a block needs a port but no element.
fn wire(c: &mut Circuit) -> NodeId {
    c.add(PhaseShifter::new(0.0))
}

/// Ring length (um) resonant at `wavelength_nm` with azimuthal order `order`.
pub fn resonant_length_um(wavelength_nm: f64, n_eff: f64, order: u32) -> f64 {
    order as f64 * wavelength_nm / 1000.0 / n_eff
}

/// Binary tree of Y-splitters: one input, `2^levels` equal outputs.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitterTree {
    pub levels: u32,
}

impl Block for SplitterTree {
    fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
        let root = wire(c);
        let mut frontier = vec![root.output(0)];
        for _ in 0..self.levels {
            let mut next = Vec::with_capacity(frontier.len() * 2);
            for out in frontier {
                let y = c.add(YSplitter);
                c.connect(out, y.input(0))?;
                next.extend([y.output(0), y.output(1)]);
            }
            frontier = next;
        }
        Ok(Ports {
            inputs: vec![root.input(0)],
            outputs: frontier,
        })
    }
}

/// Binary tree of Y-combiners: `2^levels` inputs, one output. Only the
/// in-phase part of the inputs survives, so it also computes a coherent sum.
#[derive(Clone, Debug, PartialEq)]
pub struct CombinerTree {
    pub levels: u32,
}

impl Block for CombinerTree {
    fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
        let mut inputs = Vec::new();
        let mut frontier: Vec<OutPort> = Vec::new();
        if self.levels == 0 {
            let w = wire(c);
            return Ok(Ports {
                inputs: vec![w.input(0)],
                outputs: vec![w.output(0)],
            });
        }
        for _ in 0..1usize << (self.levels - 1) {
            let y = c.add(YCombiner);
            inputs.extend([y.input(0), y.input(1)]);
            frontier.push(y.output(0));
        }
        while frontier.len() > 1 {
            let mut next = Vec::with_capacity(frontier.len() / 2);
            for pair in frontier.chunks(2) {
                let y = c.add(YCombiner);
                c.connect(pair[0], y.input(0))?;
                c.connect(pair[1], y.input(1))?;
                next.push(y.output(0));
            }
            frontier = next;
        }
        Ok(Ports {
            inputs,
            outputs: frontier,
        })
    }
}

/// Ring coupled to one bus waveguide. Ports: `in -> through`.
#[derive(Clone, Debug, PartialEq)]
pub struct AllPassRing {
    /// Power coupling between bus and ring.
    pub coupling: f64,
    pub length_um: f64,
    /// Effective index at [`Self::reference_nm`].
    pub n_eff: f64,
    pub loss_db_per_cm: f64,
    /// Group index (see [`Waveguide::n_g`]).
    pub n_g: f64,
    pub reference_nm: f64,
}

impl AllPassRing {
    fn ring(&self) -> Waveguide {
        Waveguide::new(self.length_um, self.n_eff)
            .with_loss(self.loss_db_per_cm)
            .with_group_index(self.n_g, self.reference_nm)
    }

    /// Closed-form through-port field, `(t - a e^{i phi}) / (1 - t a e^{i phi})`.
    pub fn through(&self, wavelength_nm: f64) -> C64 {
        let t = (1.0 - self.coupling).sqrt();
        let e = self.ring().transmission(wavelength_nm);
        (t - e) / (1.0 - t * e)
    }

    /// Free spectral range in nm near `wavelength_nm`: λ²/(n_g·L).
    pub fn fsr_nm(&self, wavelength_nm: f64) -> f64 {
        wavelength_nm * wavelength_nm / (self.n_g * self.length_um * 1000.0)
    }
}

impl Block for AllPassRing {
    fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
        let dc = c.add(DirectionalCoupler::new(self.coupling));
        let ring = c.add(self.ring());
        c.connect(dc.output(1), ring.input(0))?;
        c.connect(ring.output(0), dc.input(1))?;
        Ok(Ports {
            inputs: vec![dc.input(0)],
            outputs: vec![dc.output(0)],
        })
    }
}

/// Ring between two buses. Ports: `[in, add] -> [through, drop]`.
#[derive(Clone, Debug, PartialEq)]
pub struct AddDropRing {
    pub coupling_in: f64,
    pub coupling_drop: f64,
    pub length_um: f64,
    /// Effective index at [`Self::reference_nm`].
    pub n_eff: f64,
    pub loss_db_per_cm: f64,
    /// Group index (see [`Waveguide::n_g`]).
    pub n_g: f64,
    pub reference_nm: f64,
}

impl AddDropRing {
    /// Symmetric lossless ring resonant at `wavelength_nm`.
    pub fn resonant(wavelength_nm: f64, n_eff: f64, order: u32, coupling: f64) -> Self {
        Self {
            coupling_in: coupling,
            coupling_drop: coupling,
            length_um: resonant_length_um(wavelength_nm, n_eff, order),
            n_eff,
            loss_db_per_cm: 0.0,
            n_g: n_eff,
            reference_nm: wavelength_nm,
        }
    }
}

impl Block for AddDropRing {
    fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
        let half = || {
            Waveguide::new(self.length_um / 2.0, self.n_eff)
                .with_loss(self.loss_db_per_cm)
                .with_group_index(self.n_g, self.reference_nm)
        };
        let bus_in = c.add(DirectionalCoupler::new(self.coupling_in));
        let bus_drop = c.add(DirectionalCoupler::new(self.coupling_drop));
        let upper = c.add(half());
        let lower = c.add(half());
        c.connect(bus_in.output(1), upper.input(0))?;
        c.connect(upper.output(0), bus_drop.input(1))?;
        c.connect(bus_drop.output(1), lower.input(0))?;
        c.connect(lower.output(0), bus_in.input(1))?;
        Ok(Ports {
            inputs: vec![bus_in.input(0), bus_drop.input(0)],
            outputs: vec![bus_in.output(0), bus_drop.output(0)],
        })
    }
}

/// Mach-Zehnder with an extra `delay_um` in the upper arm: a periodic
/// filter that sends alternate wavelength channels to alternate outputs.
#[derive(Clone, Debug, PartialEq)]
pub struct UnbalancedMzi {
    pub delay_um: f64,
    pub n_eff: f64,
}

impl UnbalancedMzi {
    /// Fraction of input-0 power on output 1 (the cross port).
    pub fn cross_power(&self, wavelength_nm: f64) -> f64 {
        (propagation_phase(self.delay_um, self.n_eff, wavelength_nm) / 2.0)
            .cos()
            .powi(2)
    }

    pub fn fsr_nm(&self, wavelength_nm: f64) -> f64 {
        wavelength_nm * wavelength_nm / (self.n_eff * self.delay_um * 1000.0)
    }
}

impl Block for UnbalancedMzi {
    fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
        let split = c.add(DirectionalCoupler::balanced());
        let delay = c.add(Waveguide::new(self.delay_um, self.n_eff));
        let join = c.add(DirectionalCoupler::balanced());
        c.connect(split.output(0), delay.input(0))?;
        c.connect(delay.output(0), join.input(0))?;
        c.connect(split.output(1), join.input(1))?;
        Ok(Ports {
            inputs: vec![split.input(0), split.input(1)],
            outputs: vec![join.output(0), join.output(1)],
        })
    }
}

/// Tree of MZI switches routing its input to output `address`
/// (most significant bit chooses at the first level).
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchTree {
    pub levels: u32,
    pub address: usize,
}

impl Block for SwitchTree {
    fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
        let root = wire(c);
        let mut frontier = vec![root.output(0)];
        for level in 0..self.levels {
            let mut next = Vec::with_capacity(frontier.len() * 2);
            for (j, out) in frontier.into_iter().enumerate() {
                // Only the switch on the selected path matters; set the rest
                // the same way so unused branches are reproducible.
                let bit = self.address >> (self.levels - 1 - level) & 1;
                let on_path = self.address >> (self.levels - level) == j;
                let theta = if bit == 0 || !on_path { PI } else { 0.0 };
                let mzi = c.add(Mzi::new(theta, 0.0));
                c.connect(out, mzi.input(0))?;
                next.extend([mzi.output(0), mzi.output(1)]);
            }
            frontier = next;
        }
        Ok(Ports {
            inputs: vec![root.input(0)],
            outputs: frontier,
        })
    }
}

/// Unitary discrete Fourier transform, `F[j][k] = e^{-2 pi i jk/n} / sqrt(n)`.
pub fn dft_matrix(n: usize) -> Matrix {
    let norm = 1.0 / (n as f64).sqrt();
    Matrix::from_fn(n, n, |j, k| {
        C64::from_polar(norm, -2.0 * PI * (j * k % n.max(1)) as f64 / n as f64)
    })
}

/// Optical DFT on `n` modes, realised as an MZI mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct Dft {
    pub n: usize,
}

impl Dft {
    pub fn mesh(&self) -> UnitaryMesh {
        UnitaryMesh::decompose(&dft_matrix(self.n)).expect("the DFT is unitary")
    }
}

impl Block for Dft {
    fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
        self.mesh().build(c)
    }
}

/// Bus waveguide with one add-drop ring per channel.
/// Ports: `in -> [drop_0, ..., drop_{n-1}, through]`.
#[derive(Clone, Debug, PartialEq)]
pub struct WdmDemux {
    pub channels_nm: Vec<f64>,
    pub n_eff: f64,
    /// Azimuthal order of every ring; sets the free spectral range (~ lambda / order).
    pub order: u32,
    pub coupling: f64,
}

impl Block for WdmDemux {
    fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
        let entry = wire(c);
        let mut bus = entry.output(0);
        let mut outputs = Vec::with_capacity(self.channels_nm.len() + 1);
        for &wl in &self.channels_nm {
            let ring = AddDropRing::resonant(wl, self.n_eff, self.order, self.coupling).build(c)?;
            c.connect(bus, ring.inputs[0])?;
            bus = ring.outputs[0];
            outputs.push(ring.outputs[1]);
        }
        outputs.push(bus);
        Ok(Ports {
            inputs: vec![entry.input(0)],
            outputs,
        })
    }
}

/// Inject `field` on `input` and return the power on each of `outputs`.
pub fn output_powers(
    circuit: &Circuit,
    wavelength_nm: f64,
    input: InPort,
    outputs: &[OutPort],
) -> Result<Vec<f64>, CircuitError> {
    let sol = circuit.simulate(wavelength_nm, &[(input, C64::ONE)])?;
    Ok(outputs
        .iter()
        .map(|&o| sol.output_field(o).norm_sqr())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WL: f64 = 1550.0;

    fn built(block: &impl Block) -> (Circuit, Ports) {
        let mut c = Circuit::new();
        let ports = c.add_block(block).unwrap();
        (c, ports)
    }

    fn assert_close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "{a} != {b}");
    }

    #[test]
    fn resonant_length() {
        let l = resonant_length_um(1550.0, 2.0, 10);
        assert_close(l, 7.75, 1e-12);
        assert_close(
            propagation_phase(l, 2.0, 1550.0).rem_euclid(2.0 * PI),
            0.0,
            1e-9,
        );
    }

    #[test]
    fn splitter_tree_splits_equally() {
        for levels in 0..4 {
            let (c, p) = built(&SplitterTree { levels });
            assert_eq!(p.outputs.len(), 1 << levels);
            let powers = output_powers(&c, WL, p.inputs[0], &p.outputs).unwrap();
            for pw in powers {
                assert_close(pw, 1.0 / (1 << levels) as f64, 1e-12);
            }
        }
    }

    #[test]
    fn combiner_tree_sums_coherently() {
        for levels in 0..4 {
            let (c, p) = built(&CombinerTree { levels });
            let n = 1usize << levels;
            assert_eq!((p.inputs.len(), p.outputs.len()), (n, 1));
            // In phase: all power arrives.
            let drives: Vec<_> = p.inputs.iter().map(|&i| (i, C64::ONE)).collect();
            let sol = c.simulate(WL, &drives).unwrap();
            assert_close(sol.output_field(p.outputs[0]).norm_sqr(), n as f64, 1e-9);
            if n > 1 {
                // Alternating signs cancel.
                let drives: Vec<_> = p
                    .inputs
                    .iter()
                    .enumerate()
                    .map(|(k, &i)| (i, if k % 2 == 0 { C64::ONE } else { -C64::ONE }))
                    .collect();
                let sol = c.simulate(WL, &drives).unwrap();
                assert_close(sol.output_field(p.outputs[0]).norm(), 0.0, 1e-12);
            }
        }
    }

    #[test]
    fn splitter_then_combiner_is_lossless() {
        let mut c = Circuit::new();
        let s = c.add_block(&SplitterTree { levels: 3 }).unwrap();
        let k = c.add_block(&CombinerTree { levels: 3 }).unwrap();
        c.connect_all(&s.outputs, &k.inputs).unwrap();
        let p = output_powers(&c, WL, s.inputs[0], &k.outputs).unwrap();
        assert_close(p[0], 1.0, 1e-12);
    }

    #[test]
    fn all_pass_ring_matches_closed_form() {
        let ring = AllPassRing {
            coupling: 0.05,
            length_um: 30.0,
            n_eff: 2.4,
            loss_db_per_cm: 20.0,
            n_g: 2.4,
            reference_nm: 1550.0,
        };
        let (c, p) = built(&ring);
        for wl in [1549.0, 1550.0, 1550.3, 1551.7] {
            let sol = c.simulate(wl, &[(p.inputs[0], C64::ONE)]).unwrap();
            assert!((sol.output_field(p.outputs[0]) - ring.through(wl)).norm() < 1e-12);
        }
        assert_close(
            ring.fsr_nm(1550.0),
            1550.0 * 1550.0 / (2.4 * 30_000.0),
            1e-12,
        );
    }

    #[test]
    fn ring_resonances_are_spaced_by_the_group_index() {
        // A silicon-like ring (n_eff 2.4, n_g 4.2): simulated through-port
        // dips near 1550 nm sit λ²/(n_g·L) apart, not λ²/(n_eff·L).
        let ring = AllPassRing {
            coupling: 0.05,
            length_um: 50.0,
            n_eff: 2.4,
            loss_db_per_cm: 0.0,
            n_g: 4.2,
            reference_nm: 1550.0,
        };
        let (c, p) = built(&ring);
        // Lossless all-pass rings pass all power; the phase wraps fastest at
        // resonance. With a little loss, resonances become dips to find.
        let lossy = AllPassRing { loss_db_per_cm: 30.0, ..ring.clone() };
        let (cl, pl) = built(&lossy);
        let wls: Vec<f64> = (0..20001).map(|k| 1540.0 + 20.0 * k as f64 / 20000.0).collect();
        let power: Vec<f64> = wls
            .iter()
            .map(|&wl| cl.simulate(wl, &[(pl.inputs[0], C64::ONE)]).unwrap().output_field(pl.outputs[0]).norm_sqr())
            .collect();
        let dips: Vec<f64> = (1..wls.len() - 1)
            .filter(|&k| power[k] < power[k - 1] && power[k] < power[k + 1] && power[k] < 0.9)
            .map(|k| wls[k])
            .collect();
        assert!(dips.len() >= 2, "{dips:?}");
        let near = dips.windows(2).min_by(|a, b| ((a[0] + a[1]) / 2.0 - 1550.0).abs().total_cmp(&((b[0] + b[1]) / 2.0 - 1550.0).abs())).unwrap();
        let fsr = near[1] - near[0];
        assert!((fsr - ring.fsr_nm(1550.0)).abs() / ring.fsr_nm(1550.0) < 0.01, "FSR {fsr} vs {}", ring.fsr_nm(1550.0));
        assert!((ring.fsr_nm(1550.0) - 1550.0 * 1550.0 / (4.2 * 50_000.0)).abs() < 1e-12);
        // The lossless ring is still all-pass.
        let t = c.simulate(1550.0, &[(p.inputs[0], C64::ONE)]).unwrap().output_field(p.outputs[0]).norm_sqr();
        assert!((t - 1.0).abs() < 1e-9);
    }

    #[test]
    fn add_drop_ring_drops_on_resonance() {
        let ring = AddDropRing::resonant(WL, 2.4, 20, 0.1);
        let (c, p) = built(&ring);
        let on = output_powers(&c, WL, p.inputs[0], &p.outputs).unwrap();
        assert_close(on[0], 0.0, 1e-9);
        assert_close(on[1], 1.0, 1e-9);
        let fsr = WL / 20.0;
        let off = output_powers(&c, WL + fsr / 2.0, p.inputs[0], &p.outputs).unwrap();
        assert!(off[0] > 0.99 && off[1] < 0.01, "{off:?}");
        // The add port feeds the through port on resonance.
        let add = output_powers(&c, WL, p.inputs[1], &p.outputs).unwrap();
        assert_close(add[0], 1.0, 1e-9);
    }

    #[test]
    fn lossy_add_drop_ring_loses_power() {
        let ring = AddDropRing {
            loss_db_per_cm: 500.0,
            ..AddDropRing::resonant(WL, 2.4, 20, 0.1)
        };
        let (c, p) = built(&ring);
        let total: f64 = output_powers(&c, WL, p.inputs[0], &p.outputs)
            .unwrap()
            .iter()
            .sum();
        assert!(total < 0.99);
    }

    #[test]
    fn unbalanced_mzi_interleaves() {
        let mzi = UnbalancedMzi {
            delay_um: 50.0,
            n_eff: 2.0,
        };
        let (c, p) = built(&mzi);
        let fsr = mzi.fsr_nm(WL);
        for wl in [WL, WL + fsr / 4.0, WL + fsr / 2.0, WL + 0.123] {
            let pw = output_powers(&c, wl, p.inputs[0], &p.outputs).unwrap();
            assert_close(pw[1], mzi.cross_power(wl), 1e-12);
            assert_close(pw[0] + pw[1], 1.0, 1e-12);
        }
        // Where the delay is a whole number of waves, light crosses fully;
        // half an FSR away it stays in the bar port.
        let peak = 2.0 * 50.0 * 1000.0 / (2.0f64 * 50.0 * 1000.0 / 1550.0).round();
        assert_close(mzi.cross_power(peak), 1.0, 1e-9);
        assert_close(mzi.cross_power(peak + mzi.fsr_nm(peak) / 2.0), 0.0, 1e-3);
    }

    #[test]
    fn switch_tree_routes_every_address() {
        for levels in 1..4 {
            for address in 0..1usize << levels {
                let (c, p) = built(&SwitchTree { levels, address });
                let pw = output_powers(&c, WL, p.inputs[0], &p.outputs).unwrap();
                for (k, v) in pw.iter().enumerate() {
                    let expected = if k == address { 1.0 } else { 0.0 };
                    assert_close(*v, expected, 1e-12);
                }
            }
        }
    }

    #[test]
    fn dft_matrix_is_unitary_fourier() {
        let f = dft_matrix(4);
        assert!((f.adjoint() * &f - Matrix::identity(4, 4)).norm_l2() < 1e-12);
        assert!((f[(1, 1)] - C64::new(0.0, -0.5)).norm() < 1e-12);
        assert_eq!(dft_matrix(1)[(0, 0)], C64::ONE);
    }

    #[test]
    fn optical_dft_transforms_signals() {
        let n = 8;
        let dft = Dft { n };
        assert_eq!(dft.mesh().mzis.len(), n * (n - 1) / 2);
        let (c, p) = built(&dft);
        let t = c.transfer_matrix(WL, &p.inputs, &p.outputs).unwrap();
        assert!((t - dft_matrix(n)).norm_l2() < 1e-9);

        // A pure tone e^{+2 pi i 3 j / n} lands on output 3.
        let drives: Vec<_> = p
            .inputs
            .iter()
            .enumerate()
            .map(|(j, &i)| {
                (
                    i,
                    C64::from_polar(1.0, 2.0 * PI * 3.0 * j as f64 / n as f64),
                )
            })
            .collect();
        let sol = c.simulate(WL, &drives).unwrap();
        for (k, &o) in p.outputs.iter().enumerate() {
            let expected = if k == 3 { n as f64 } else { 0.0 };
            assert_close(sol.output_field(o).norm_sqr(), expected, 1e-9);
        }
    }

    #[test]
    fn wdm_demux_separates_channels() {
        let demux = WdmDemux {
            channels_nm: vec![1540.0, 1550.0, 1560.0],
            n_eff: 2.4,
            order: 20,
            // Linewidth ~ FSR * coupling / pi ~ 1.2 nm, well inside the 10 nm grid.
            coupling: 0.05,
        };
        let (c, p) = built(&demux);
        assert_eq!(p.outputs.len(), 4);
        for (ch, &wl) in demux.channels_nm.iter().enumerate() {
            let pw = output_powers(&c, wl, p.inputs[0], &p.outputs).unwrap();
            assert!(pw[ch] > 0.95, "channel {ch}: {pw:?}");
            for (k, v) in pw.iter().enumerate() {
                if k != ch {
                    assert!(*v < 0.05, "crosstalk into {k} at channel {ch}: {pw:?}");
                }
            }
        }
        // Between channels, light continues on the bus.
        let pw = output_powers(&c, 1545.0, p.inputs[0], &p.outputs).unwrap();
        assert!(pw[3] > 0.9, "{pw:?}");
    }
}
