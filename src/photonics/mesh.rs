//! Programmable interferometer meshes.
//!
//! [`UnitaryMesh`] realises any unitary with a triangular (Reck) arrangement
//! of [`Mzi`]s followed by output phase shifters. [`SvdMesh`] realises an
//! arbitrary matrix as `M = U S V^H`: a mesh for `V^H`, one attenuator per
//! mode for `S` and a mesh for `U` — the building block of optical matrix
//! multipliers and neural network layers.

use std::f64::consts::PI;
use std::fmt;

use super::circuit::{Block, Circuit, CircuitError, InPort, NodeId, OutPort, Ports};
use super::{Attenuator, C64, Matrix, Mzi, PhaseShifter};

#[derive(Clone, Debug, PartialEq)]
pub enum MeshError {
    NotSquare,
    /// The matrix deviates from unitarity by this Frobenius norm.
    NotUnitary(f64),
    Svd,
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSquare => write!(f, "matrix is not square"),
            Self::NotUnitary(e) => write!(f, "matrix is not unitary (deviation {e:.3e})"),
            Self::Svd => write!(f, "singular value decomposition did not converge"),
        }
    }
}

impl std::error::Error for MeshError {}

/// One MZI acting on modes `mode` and `mode + 1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MziSetting {
    pub mode: usize,
    pub theta: f64,
    pub phi: f64,
}

/// `U = D * T_K * ... * T_1`: light passes `mzis[0]` first, then the output
/// phases `D`.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitaryMesh {
    pub size: usize,
    pub mzis: Vec<MziSetting>,
    pub output_phases: Vec<f64>,
}

/// Write a 2×2 unitary `v` as `diag(top, bottom) · Mzi(θ, φ)`; returns
/// `(θ, φ, top, bottom)`. Where φ doesn't matter (θ = 0 or π) it is 0.
fn factor(v: [[C64; 2]; 2]) -> (f64, f64, C64, C64) {
    let (s, c) = (v[0][0].norm(), v[0][1].norm());
    let theta = 2.0 * s.atan2(c);
    let g = C64::new(0.0, 1.0) * C64::from_polar(1.0, theta / 2.0);
    let (sin, cos) = (theta / 2.0).sin_cos();
    let unit = |z: C64| if z.norm() > 0.0 { z / z.norm() } else { C64::new(1.0, 0.0) };
    const EPS: f64 = 1e-12;
    if cos <= EPS {
        // The bar state: v = diag(a g e^{iφ}, −b g) with φ = 0.
        (theta, 0.0, unit(v[0][0] / g), unit(-v[1][1] / g))
    } else if sin <= EPS {
        // The cross state: v = [[0, a g], [b g, 0]] with φ = 0.
        (theta, 0.0, unit(v[0][1] / g), unit(v[1][0] / g))
    } else {
        let top = unit(v[0][1] / (g * cos));
        let phi = (v[0][0] / (top * g * sin)).arg();
        (theta, phi, top, unit(-v[1][1] / (g * sin)))
    }
}

/// Left-multiply rows `m`, `m + 1` of `a` by the MZI transfer matrix.
fn apply_mzi(a: &mut Matrix, s: MziSetting) {
    let t = Mzi::matrix(s.theta, s.phi);
    let m = s.mode;
    for k in 0..a.ncols() {
        let (x, y) = (a[(m, k)], a[(m + 1, k)]);
        a[(m, k)] = t[(0, 0)] * x + t[(0, 1)] * y;
        a[(m + 1, k)] = t[(1, 0)] * x + t[(1, 1)] * y;
    }
}

impl UnitaryMesh {
    pub fn decompose(u: &Matrix) -> Result<Self, MeshError> {
        let n = u.nrows();
        if u.ncols() != n {
            return Err(MeshError::NotSquare);
        }
        let deviation = (u.adjoint() * u - Matrix::identity(n, n)).norm_l2();
        if deviation > 1e-8 * (n.max(1) as f64) {
            return Err(MeshError::NotUnitary(deviation));
        }

        // Null U^H column by column from the bottom: T_K ... T_1 U^H = D,
        // hence U = D^H T_K ... T_1.
        let mut a = u.adjoint().to_owned();
        let mut mzis = Vec::with_capacity(n * n.saturating_sub(1) / 2);
        for col in 0..n.saturating_sub(1) {
            for m in (col..n - 1).rev() {
                let (x, y) = (a[(m, col)], a[(m + 1, col)]);
                let s = MziSetting {
                    mode: m,
                    theta: 2.0 * x.norm().atan2(y.norm()),
                    phi: y.arg() - x.arg(),
                };
                apply_mzi(&mut a, s);
                mzis.push(s);
            }
        }
        let output_phases = (0..n).map(|i| -a[(i, i)].arg()).collect();
        Ok(Self {
            size: n,
            mzis,
            output_phases,
        })
    }

    /// The rectangular (Clements) arrangement of `u`: the same MZIs and
    /// output phases as [`Self::decompose`], but `n` columns deep instead of
    /// `2n − 3`, and every path crosses the same number of MZIs, so losses
    /// stay balanced. It's what programmable photonic processors are built
    /// as (W. R. Clements et al., Optica 3, 1460, 2016).
    pub fn decompose_rectangular(u: &Matrix) -> Result<Self, MeshError> {
        let n = u.nrows();
        if u.ncols() != n {
            return Err(MeshError::NotSquare);
        }
        let deviation = (u.adjoint() * u - Matrix::identity(n, n)).norm_l2();
        if deviation > 1e-8 * (n.max(1) as f64) {
            return Err(MeshError::NotUnitary(deviation));
        }
        // Null the lower-left triangle along anti-diagonals, alternately from
        // the right (columns: an MZI the light meets first) and from the left
        // (rows: one it meets last): L_p … L_1 U R_1^H … R_q^H = D.
        let mut a = u.to_owned();
        let mut first: Vec<MziSetting> = Vec::new();
        let mut last: Vec<MziSetting> = Vec::new();
        for i in 0..n.saturating_sub(1) {
            if i % 2 == 0 {
                for j in 0..=i {
                    let (r, m) = (n - 1 - j, i - j);
                    let (x, y) = (a[(r, m)], a[(r, m + 1)]);
                    // (U M^H)[r, m] = 0.
                    let s = MziSetting { mode: m, theta: 2.0 * y.norm().atan2(x.norm()), phi: x.arg() - y.arg() - PI };
                    let t = Mzi::matrix(s.theta, s.phi);
                    for row in 0..n {
                        let (p, q) = (a[(row, m)], a[(row, m + 1)]);
                        a[(row, m)] = p * t[(0, 0)].conj() + q * t[(0, 1)].conj();
                        a[(row, m + 1)] = p * t[(1, 0)].conj() + q * t[(1, 1)].conj();
                    }
                    first.push(s);
                }
            } else {
                for j in 0..=i {
                    let (r, c) = (n + j - i - 2, j);
                    let (x, y) = (a[(r, c)], a[(r + 1, c)]);
                    // (M U)[r + 1, c] = 0.
                    let s = MziSetting { mode: r, theta: 2.0 * x.norm().atan2(y.norm()), phi: y.arg() - x.arg() };
                    apply_mzi(&mut a, s);
                    last.push(s);
                }
            }
        }
        // U = L_1^H … L_p^H D R_q … R_1. Move D to the outputs through each
        // L^H in turn: L^H D = D' M', with M' an MZI on the same modes.
        let mut d: Vec<C64> = (0..n).map(|i| a[(i, i)]).collect();
        let mut moved = Vec::with_capacity(last.len());
        for s in last.iter().rev() {
            let t = Mzi::matrix(s.theta, s.phi);
            let m = s.mode;
            // V = L^H diag(d_m, d_m+1).
            let v = [
                [t[(0, 0)].conj() * d[m], t[(1, 0)].conj() * d[m + 1]],
                [t[(0, 1)].conj() * d[m], t[(1, 1)].conj() * d[m + 1]],
            ];
            let (theta, phi, top, bottom) = factor(v);
            d[m] = top;
            d[m + 1] = bottom;
            moved.push(MziSetting { mode: m, theta, phi });
        }
        // D M'_1 … M'_p R_q … R_1: the light meets R_1 … R_q, then M'_p …
        // M'_1, in the order they were found.
        let mut mzis = first;
        mzis.extend(moved);
        let output_phases = d.iter().map(|z| z.arg()).collect();
        Ok(Self { size: n, mzis, output_phases })
    }

    /// How many columns deep the mesh is, each MZI as early as its modes
    /// allow.
    pub fn depth(&self) -> usize {
        self.columns().iter().copied().max().map_or(0, |c| c + 1)
    }

    /// The column each MZI sits in, as early as its modes allow.
    pub fn columns(&self) -> Vec<usize> {
        let mut free = vec![0usize; self.size];
        self.mzis
            .iter()
            .map(|s| {
                let c = free[s.mode].max(free[s.mode + 1]);
                free[s.mode] = c + 1;
                free[s.mode + 1] = c + 1;
                c
            })
            .collect()
    }

    /// The unitary implemented by the mesh.
    pub fn matrix(&self) -> Matrix {
        let mut m = Matrix::identity(self.size, self.size);
        for &s in &self.mzis {
            apply_mzi(&mut m, s);
        }
        for (i, &p) in self.output_phases.iter().enumerate() {
            let d = C64::from_polar(1.0, p);
            for k in 0..self.size {
                m[(i, k)] *= d;
            }
        }
        m
    }
}

/// Tracks the open end of each waveguide mode while a mesh is being built.
pub struct ModeWiring {
    entry: Vec<Option<InPort>>,
    front: Vec<Option<OutPort>>,
}

impl ModeWiring {
    pub fn new(modes: usize) -> Self {
        Self {
            entry: vec![None; modes],
            front: vec![None; modes],
        }
    }

    /// Append `node` to the given modes: its input/output `k` sits on `modes[k]`.
    pub fn attach(
        &mut self,
        circuit: &mut Circuit,
        node: NodeId,
        modes: &[usize],
    ) -> Result<(), CircuitError> {
        for (k, &m) in modes.iter().enumerate() {
            match self.front[m] {
                Some(out) => circuit.connect(out, node.input(k))?,
                None => self.entry[m] = Some(node.input(k)),
            }
            self.front[m] = Some(node.output(k));
        }
        Ok(())
    }

    /// Boundary ports; every mode must have had at least one element attached.
    pub fn finish(self) -> Ports {
        Ports {
            inputs: self
                .entry
                .into_iter()
                .map(|p| p.expect("empty mode"))
                .collect(),
            outputs: self
                .front
                .into_iter()
                .map(|p| p.expect("empty mode"))
                .collect(),
        }
    }
}

impl Block for UnitaryMesh {
    fn build(&self, circuit: &mut Circuit) -> Result<Ports, CircuitError> {
        let mut wiring = ModeWiring::new(self.size);
        for s in &self.mzis {
            let node = circuit.add(Mzi::new(s.theta, s.phi));
            wiring.attach(circuit, node, &[s.mode, s.mode + 1])?;
        }
        for (i, &p) in self.output_phases.iter().enumerate() {
            let node = circuit.add(PhaseShifter::new(p));
            wiring.attach(circuit, node, &[i])?;
        }
        Ok(wiring.finish())
    }
}

/// Passive realisation of an arbitrary `rows x cols` matrix.
///
/// The optical circuit computes `M x / scale`; multiply detected fields by
/// [`SvdMesh::scale`] to recover `M x`.
#[derive(Clone, Debug, PartialEq)]
pub struct SvdMesh {
    pub rows: usize,
    pub cols: usize,
    pub v_adjoint: UnitaryMesh,
    /// Per-mode amplitude transmission `sigma_i / sigma_max`.
    pub attenuation: Vec<f64>,
    pub u: UnitaryMesh,
    /// Largest singular value.
    pub scale: f64,
}

impl SvdMesh {
    pub fn decompose(m: &Matrix) -> Result<Self, MeshError> {
        let (rows, cols) = (m.nrows(), m.ncols());
        let n = rows.max(cols);
        let padded = Matrix::from_fn(n, n, |i, j| {
            if i < rows && j < cols {
                m[(i, j)]
            } else {
                C64::ZERO
            }
        });
        let svd = padded.svd().map_err(|_| MeshError::Svd)?;
        let sigma = svd.S().column_vector();
        let scale = (0..n).map(|i| sigma[i].re).fold(0.0, f64::max);
        let attenuation = (0..n)
            .map(|i| {
                if scale > 0.0 {
                    sigma[i].re / scale
                } else {
                    0.0
                }
            })
            .collect();
        Ok(Self {
            rows,
            cols,
            v_adjoint: UnitaryMesh::decompose(&svd.V().adjoint().to_owned())?,
            attenuation,
            u: UnitaryMesh::decompose(&svd.U().to_owned())?,
            scale,
        })
    }

    /// The matrix implemented (including `scale`), cropped to `rows x cols`.
    pub fn matrix(&self) -> Matrix {
        let n = self.attenuation.len();
        let s = Matrix::from_fn(n, n, |i, j| {
            if i == j {
                C64::new(self.attenuation[i] * self.scale, 0.0)
            } else {
                C64::ZERO
            }
        });
        let full = self.u.matrix() * s * self.v_adjoint.matrix();
        Matrix::from_fn(self.rows, self.cols, |i, j| full[(i, j)])
    }
}

impl Block for SvdMesh {
    /// Exposes `cols` inputs and `rows` outputs; padding modes stay open.
    fn build(&self, circuit: &mut Circuit) -> Result<Ports, CircuitError> {
        let first = self.v_adjoint.build(circuit)?;
        let attenuators: Vec<NodeId> = self
            .attenuation
            .iter()
            .map(|&a| circuit.add(Attenuator::from_amplitude(a)))
            .collect();
        let atten_in: Vec<InPort> = attenuators.iter().map(|n| n.input(0)).collect();
        let atten_out: Vec<OutPort> = attenuators.iter().map(|n| n.output(0)).collect();
        circuit.connect_all(&first.outputs, &atten_in)?;
        let second = self.u.build(circuit)?;
        circuit.connect_all(&atten_out, &second.inputs)?;
        Ok(Ports {
            inputs: first.inputs[..self.cols].to_vec(),
            outputs: second.outputs[..self.rows].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random complex matrix.
    fn pseudo_random(rows: usize, cols: usize, seed: u64) -> Matrix {
        let mut state = seed;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        Matrix::from_fn(rows, cols, |_, _| C64::new(next(), next()))
    }

    #[test]
    fn unitary_mesh_reconstructs() {
        for n in [1, 2, 3, 6] {
            let u = pseudo_random(n, n, n as u64).svd().unwrap().U().to_owned();
            let mesh = UnitaryMesh::decompose(&u).unwrap();
            assert_eq!(mesh.mzis.len(), n * (n - 1) / 2);
            assert!((mesh.matrix() - &u).norm_l2() < 1e-10);
        }
    }

    #[test]
    fn rectangular_mesh_reconstructs_n_columns_deep() {
        for n in [1, 2, 3, 4, 5, 8, 16, 33] {
            let u = pseudo_random(n, n, 40 + n as u64).svd().unwrap().U().to_owned();
            let mesh = UnitaryMesh::decompose_rectangular(&u).unwrap();
            assert_eq!(mesh.mzis.len(), n * (n - 1) / 2);
            let err = (mesh.matrix() - &u).norm_l2();
            assert!(err < 1e-9, "n = {n}: {err}");
            if n > 2 {
                assert_eq!(mesh.depth(), n, "Clements is n columns deep");
            }
        }
    }

    #[test]
    fn rectangular_mesh_programs_switching_states() {
        // The DFT, the identity and a permutation hit θ = 0 and π exactly.
        let n = 8;
        let dft = Matrix::from_fn(n, n, |r, c| C64::from_polar(1.0 / (n as f64).sqrt(), -2.0 * PI * (r * c) as f64 / n as f64));
        let identity = Matrix::identity(n, n);
        let reverse = Matrix::from_fn(n, n, |r, c| if r + c == n - 1 { C64::new(1.0, 0.0) } else { C64::new(0.0, 0.0) });
        for (name, u) in [("DFT", dft), ("identity", identity), ("reversal", reverse)] {
            let mesh = UnitaryMesh::decompose_rectangular(&u).unwrap();
            assert!((mesh.matrix() - &u).norm_l2() < 1e-9, "{name}");
        }
    }

    #[test]
    fn rejects_non_unitary() {
        let m = pseudo_random(3, 3, 7);
        assert!(matches!(
            UnitaryMesh::decompose(&m),
            Err(MeshError::NotUnitary(_))
        ));
    }

    #[test]
    fn mesh_circuit_matches_matrix() {
        let u = pseudo_random(4, 4, 42).svd().unwrap().U().to_owned();
        let mesh = UnitaryMesh::decompose(&u).unwrap();
        let mut c = Circuit::new();
        let ports = c.add_block(&mesh).unwrap();
        let t = c
            .transfer_matrix(1550.0, &ports.inputs, &ports.outputs)
            .unwrap();
        assert!((t - &u).norm_l2() < 1e-10);
    }

    #[test]
    fn svd_mesh_multiplies_matrix() {
        // Same case as the original test_pure.js prototype.
        let w = [
            [1.0, 2.0, 3.0, 4.0],
            [0.0, 1.0, 0.0, 2.0],
            [0.0, 0.0, 1.0, 3.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let w = Matrix::from_fn(4, 4, |i, j| C64::new(w[i][j], 0.0));
        let mesh = SvdMesh::decompose(&w).unwrap();
        assert!((mesh.matrix() - &w).norm_l2() < 1e-10);

        let mut c = Circuit::new();
        let ports = c.add_block(&mesh).unwrap();
        let drives: Vec<_> = ports
            .inputs
            .iter()
            .map(|&p| (p, C64::new(1.0, 0.0)))
            .collect();
        let sol = c.simulate(1550.0, &drives).unwrap();
        let expected = [10.0, 3.0, 4.0, 1.0];
        for (port, e) in ports.outputs.iter().zip(expected) {
            let y = sol.output_field(*port) * mesh.scale;
            assert!((y - C64::new(e, 0.0)).norm() < 1e-9, "{y} != {e}");
        }
    }

    #[test]
    fn svd_mesh_handles_rectangular() {
        let m = pseudo_random(2, 5, 3);
        let mesh = SvdMesh::decompose(&m).unwrap();
        let mut c = Circuit::new();
        let ports = c.add_block(&mesh).unwrap();
        assert_eq!((ports.inputs.len(), ports.outputs.len()), (5, 2));
        let t = c
            .transfer_matrix(1550.0, &ports.inputs, &ports.outputs)
            .unwrap();
        let scaled = Matrix::from_fn(2, 5, |i, j| t[(i, j)] * mesh.scale);
        assert!((scaled - &m).norm_l2() < 1e-9);
    }

    #[test]
    fn rejects_non_square() {
        assert_eq!(
            UnitaryMesh::decompose(&Matrix::zeros(2, 3)),
            Err(MeshError::NotSquare)
        );
    }

    #[test]
    fn errors_display() {
        assert_eq!(MeshError::NotSquare.to_string(), "matrix is not square");
        assert_eq!(
            MeshError::NotUnitary(0.5).to_string(),
            "matrix is not unitary (deviation 5.000e-1)"
        );
        assert_eq!(
            MeshError::Svd.to_string(),
            "singular value decomposition did not converge"
        );
        let _: &dyn std::error::Error = &MeshError::Svd;
    }

    #[test]
    fn trivial_meshes() {
        let empty = UnitaryMesh::decompose(&Matrix::zeros(0, 0)).unwrap();
        assert!(empty.mzis.is_empty() && empty.output_phases.is_empty());

        let phase = Matrix::full(1, 1, C64::from_polar(1.0, 0.7));
        let mesh = UnitaryMesh::decompose(&phase).unwrap();
        assert!(mesh.mzis.is_empty());
        assert!((mesh.output_phases[0] - 0.7).abs() < 1e-12);

        let id = UnitaryMesh::decompose(&Matrix::identity(4, 4)).unwrap();
        assert!((id.matrix() - Matrix::identity(4, 4)).norm_l2() < 1e-12);
    }

    #[test]
    fn permutation_uses_cross_states() {
        let swap = Matrix::from_fn(2, 2, |i, j| if i != j { C64::ONE } else { C64::ZERO });
        let mesh = UnitaryMesh::decompose(&swap).unwrap();
        assert_eq!(mesh.mzis.len(), 1);
        assert!((mesh.matrix() - &swap).norm_l2() < 1e-12);
    }

    #[test]
    fn mzi_settings_are_ordered_and_in_range() {
        let u = pseudo_random(5, 5, 9).svd().unwrap().U().to_owned();
        let mesh = UnitaryMesh::decompose(&u).unwrap();
        assert_eq!(mesh.size, 5);
        for s in &mesh.mzis {
            assert!(s.mode + 1 < mesh.size);
            assert!((0.0..=std::f64::consts::PI).contains(&s.theta));
        }
        // Reck order: the first column is cleared from the bottom pair up.
        let modes: Vec<usize> = mesh.mzis.iter().take(4).map(|s| s.mode).collect();
        assert_eq!(modes, vec![3, 2, 1, 0]);
    }

    #[test]
    fn mode_wiring_chains_elements() {
        let mut c = Circuit::new();
        let mut wiring = ModeWiring::new(3);
        let mzi = c.add(Mzi::new(0.0, 0.0));
        wiring.attach(&mut c, mzi, &[1, 2]).unwrap();
        let ps: Vec<NodeId> = (0..3).map(|_| c.add(PhaseShifter::new(0.0))).collect();
        for (m, &p) in ps.iter().enumerate() {
            wiring.attach(&mut c, p, &[m]).unwrap();
        }
        let ports = wiring.finish();
        assert_eq!(
            ports.inputs,
            vec![ps[0].input(0), mzi.input(0), mzi.input(1)]
        );
        assert_eq!(
            ports.outputs,
            ps.iter().map(|p| p.output(0)).collect::<Vec<_>>()
        );
        assert_eq!(c.sink_of(mzi.output(0)), Some(ps[1].input(0)));
        assert_eq!(c.sink_of(mzi.output(1)), Some(ps[2].input(0)));
    }

    #[test]
    #[should_panic(expected = "empty mode")]
    fn mode_wiring_requires_every_mode() {
        let mut c = Circuit::new();
        let mut wiring = ModeWiring::new(2);
        let p = c.add(PhaseShifter::new(0.0));
        wiring.attach(&mut c, p, &[0]).unwrap();
        wiring.finish();
    }

    #[test]
    fn mode_wiring_propagates_connect_errors() {
        let mut c = Circuit::new();
        let mut wiring = ModeWiring::new(1);
        let a = c.add(PhaseShifter::new(0.0));
        let b = c.add(PhaseShifter::new(0.0));
        wiring.attach(&mut c, a, &[0]).unwrap();
        wiring.attach(&mut c, b, &[0]).unwrap();
        // Re-attaching `b` would feed its output into its already-used input.
        assert_eq!(
            wiring.attach(&mut c, b, &[0]),
            Err(CircuitError::InputAlreadyConnected(b.input(0)))
        );
    }

    #[test]
    fn unitary_mesh_block_shape() {
        let u = pseudo_random(3, 3, 11).svd().unwrap().U().to_owned();
        let mesh = UnitaryMesh::decompose(&u).unwrap();
        let mut c = Circuit::new();
        let ports = c.add_block(&mesh).unwrap();
        assert_eq!((ports.inputs.len(), ports.outputs.len()), (3, 3));
        // 3 MZIs + 3 output phase shifters.
        assert_eq!(c.len(), 6);
        assert_eq!(c.open_inputs().count(), 3);
        assert_eq!(c.open_outputs().count(), 3);
    }

    #[test]
    fn svd_mesh_of_zero_matrix_is_dark() {
        let mesh = SvdMesh::decompose(&Matrix::zeros(3, 3)).unwrap();
        assert_eq!(mesh.scale, 0.0);
        assert!(mesh.attenuation.iter().all(|&a| a == 0.0));
        assert!(mesh.matrix().norm_l2() < 1e-12);
    }

    #[test]
    fn svd_mesh_handles_tall_and_attenuation_is_passive() {
        let m = pseudo_random(4, 2, 5);
        let mesh = SvdMesh::decompose(&m).unwrap();
        assert_eq!((mesh.rows, mesh.cols), (4, 2));
        assert!((mesh.matrix() - &m).norm_l2() < 1e-10);
        assert!(
            mesh.attenuation
                .iter()
                .all(|&a| (0.0..=1.0 + 1e-12).contains(&a))
        );
        assert!(mesh.attenuation.iter().any(|&a| (a - 1.0).abs() < 1e-12));

        let mut c = Circuit::new();
        let ports = c.add_block(&mesh).unwrap();
        assert_eq!((ports.inputs.len(), ports.outputs.len()), (2, 4));
        let t = c
            .transfer_matrix(1550.0, &ports.inputs, &ports.outputs)
            .unwrap();
        let scaled = Matrix::from_fn(4, 2, |i, j| t[(i, j)] * mesh.scale);
        assert!((scaled - &m).norm_l2() < 1e-9);
    }
}
