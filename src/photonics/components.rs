//! Basic optical building blocks.

use std::f64::consts::PI;
use std::fmt::Debug;

use super::{C64, Matrix};

const I: C64 = C64::new(0.0, 1.0);

/// A linear optical element.
pub trait Component: Debug + Send + Sync {
    fn name(&self) -> &str;
    fn num_inputs(&self) -> usize;
    fn num_outputs(&self) -> usize;

    /// Scattering matrix of shape `num_outputs x num_inputs` at `wavelength_nm`.
    fn s_matrix(&self, wavelength_nm: f64) -> Matrix;

    /// Field emitted on each output port at `wavelength_nm`, for sources.
    fn emission(&self, _wavelength_nm: f64) -> Option<Vec<C64>> {
        None
    }

    /// Wavelengths this component emits at, for sources.
    fn emission_wavelengths(&self) -> Vec<f64> {
        Vec::new()
    }

    /// Which light source a source's emission comes from. Sources with the
    /// same id are mutually coherent (phase-locked, like light split from
    /// one laser); different ids are independent, so they don't interfere
    /// and their powers add.
    fn source_id(&self) -> u32 {
        0
    }
}

/// Amplitude transmission for a loss given in dB.
pub fn db_to_amplitude(loss_db: f64) -> f64 {
    10f64.powf(-loss_db / 20.0)
}

/// Propagation phase `2 pi n L / lambda` for a length in um and wavelength in nm.
pub fn propagation_phase(length_um: f64, n_eff: f64, wavelength_nm: f64) -> f64 {
    2.0 * PI * n_eff * length_um * 1000.0 / wavelength_nm
}

fn scalar(value: C64) -> Matrix {
    Matrix::full(1, 1, value)
}

/// Builds a matrix from row-major data.
pub fn matrix(rows: usize, cols: usize, data: &[C64]) -> Matrix {
    assert_eq!(data.len(), rows * cols);
    Matrix::from_fn(rows, cols, |i, j| data[i * cols + j])
}

/// Speed of light in vacuum, m/s.
pub const SPEED_OF_LIGHT: f64 = 299_792_458.0;

/// Default reference wavelength for dispersion: the telecom C band.
pub const REFERENCE_NM: f64 = 1550.0;

/// Effective index at `wavelength_nm`, to first order around `reference_nm`
/// where it is `n_eff` and the group index is `n_g`:
/// `n_eff(λ) = n_eff − (n_g − n_eff)·(λ − λ₀)/λ₀`. With `n_g = n_eff` the
/// index is constant.
pub fn effective_index(n_eff: f64, n_g: f64, reference_nm: f64, wavelength_nm: f64) -> f64 {
    n_eff - (n_g - n_eff) * (wavelength_nm - reference_nm) / reference_nm
}

/// A straight section of waveguide (or free space with `n_eff = 1`).
#[derive(Clone, Debug, PartialEq)]
pub struct Waveguide {
    pub length_um: f64,
    /// Effective index at [`Self::reference_nm`].
    pub n_eff: f64,
    pub loss_db_per_cm: f64,
    /// Group index: sets how the phase changes with wavelength, and so ring
    /// spacing (FSR = λ²/(n_g·L)) and delay (n_g·L/c).
    pub n_g: f64,
    pub reference_nm: f64,
}

impl Waveguide {
    /// A waveguide without extra dispersion (`n_g = n_eff`).
    pub fn new(length_um: f64, n_eff: f64) -> Self {
        Self {
            length_um,
            n_eff,
            loss_db_per_cm: 0.0,
            n_g: n_eff,
            reference_nm: REFERENCE_NM,
        }
    }

    /// Sets the group index, with `n_eff` holding at `reference_nm`.
    pub fn with_group_index(mut self, n_g: f64, reference_nm: f64) -> Self {
        self.n_g = n_g;
        self.reference_nm = reference_nm;
        self
    }

    /// Effective index at `wavelength_nm`.
    pub fn index_at(&self, wavelength_nm: f64) -> f64 {
        effective_index(self.n_eff, self.n_g, self.reference_nm, wavelength_nm)
    }

    /// Time light takes to cross it, in picoseconds.
    pub fn group_delay_ps(&self) -> f64 {
        self.n_g * self.length_um * 1e-6 / SPEED_OF_LIGHT * 1e12
    }

    pub fn free_space(length_um: f64) -> Self {
        Self::new(length_um, 1.0)
    }

    pub fn with_loss(mut self, loss_db_per_cm: f64) -> Self {
        self.loss_db_per_cm = loss_db_per_cm;
        self
    }

    pub fn transmission(&self, wavelength_nm: f64) -> C64 {
        let loss_db = self.loss_db_per_cm * self.length_um * 1e-4;
        C64::from_polar(
            db_to_amplitude(loss_db),
            propagation_phase(self.length_um, self.index_at(wavelength_nm), wavelength_nm),
        )
    }
}

impl Component for Waveguide {
    fn name(&self) -> &str {
        "Waveguide"
    }
    fn num_inputs(&self) -> usize {
        1
    }
    fn num_outputs(&self) -> usize {
        1
    }
    fn s_matrix(&self, wavelength_nm: f64) -> Matrix {
        scalar(self.transmission(wavelength_nm))
    }
}

/// Adds a fixed phase (e.g. a thermo-optic heater), independent of wavelength.
#[derive(Clone, Debug, PartialEq)]
pub struct PhaseShifter {
    pub phase: f64,
}

impl PhaseShifter {
    pub fn new(phase: f64) -> Self {
        Self { phase }
    }
}

impl Component for PhaseShifter {
    fn name(&self) -> &str {
        "PhaseShifter"
    }
    fn num_inputs(&self) -> usize {
        1
    }
    fn num_outputs(&self) -> usize {
        1
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        scalar(C64::from_polar(1.0, self.phase))
    }
}

/// Wavelength-independent loss.
#[derive(Clone, Debug, PartialEq)]
pub struct Attenuator {
    /// Amplitude transmission in `[0, 1]`.
    pub amplitude: f64,
}

impl Attenuator {
    pub fn from_db(loss_db: f64) -> Self {
        Self {
            amplitude: db_to_amplitude(loss_db),
        }
    }

    pub fn from_power_transmission(transmission: f64) -> Self {
        Self {
            amplitude: transmission.clamp(0.0, 1.0).sqrt(),
        }
    }

    pub fn from_amplitude(amplitude: f64) -> Self {
        Self { amplitude }
    }
}

impl Component for Attenuator {
    fn name(&self) -> &str {
        "Attenuator"
    }
    fn num_inputs(&self) -> usize {
        1
    }
    fn num_outputs(&self) -> usize {
        1
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        scalar(C64::new(self.amplitude, 0.0))
    }
}

/// Lossless 2x2 coupler; also models a free-space beam splitter.
///
/// Ports: input `i` goes straight through to output `i` with amplitude `t`,
/// and crosses to the other output with amplitude `i k`.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectionalCoupler {
    /// Fraction of power coupled to the cross port, in `[0, 1]`.
    pub coupling: f64,
}

pub type BeamSplitter = DirectionalCoupler;

impl DirectionalCoupler {
    pub fn new(coupling: f64) -> Self {
        Self { coupling }
    }

    pub fn balanced() -> Self {
        Self::new(0.5)
    }
}

impl Component for DirectionalCoupler {
    fn name(&self) -> &str {
        "DirectionalCoupler"
    }
    fn num_inputs(&self) -> usize {
        2
    }
    fn num_outputs(&self) -> usize {
        2
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        let k = self.coupling.clamp(0.0, 1.0);
        let t = C64::new((1.0 - k).sqrt(), 0.0);
        let x = I * k.sqrt();
        matrix(2, 2, &[t, x, x, t])
    }
}

/// Mach-Zehnder interferometer: phase `phi` on input 0, a 50:50 coupler, an
/// internal phase `theta` on the upper arm and a second 50:50 coupler.
///
/// `theta = 0` is the cross state, `theta = pi` the bar state.
#[derive(Clone, Debug, PartialEq)]
pub struct Mzi {
    pub theta: f64,
    pub phi: f64,
}

impl Mzi {
    pub fn new(theta: f64, phi: f64) -> Self {
        Self { theta, phi }
    }

    /// `i e^{i theta/2} [[e^{i phi} sin(theta/2), cos(theta/2)], [e^{i phi} cos(theta/2), -sin(theta/2)]]`
    pub fn matrix(theta: f64, phi: f64) -> Matrix {
        let g = I * C64::from_polar(1.0, theta / 2.0);
        let e = C64::from_polar(1.0, phi);
        let (s, c) = (theta / 2.0).sin_cos();
        matrix(2, 2, &[g * e * s, g * c, g * e * c, -g * s])
    }
}

impl Component for Mzi {
    fn name(&self) -> &str {
        "Mzi"
    }
    fn num_inputs(&self) -> usize {
        2
    }
    fn num_outputs(&self) -> usize {
        2
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        Self::matrix(self.theta, self.phi)
    }
}

/// 1x2 Y-junction splitting power equally.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct YSplitter;

impl Component for YSplitter {
    fn name(&self) -> &str {
        "YSplitter"
    }
    fn num_inputs(&self) -> usize {
        1
    }
    fn num_outputs(&self) -> usize {
        2
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        Matrix::full(2, 1, C64::new(0.5f64.sqrt(), 0.0))
    }
}

/// 2x1 Y-junction. Only the symmetric (in-phase) part of the inputs is guided,
/// the antisymmetric part radiates away.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct YCombiner;

impl Component for YCombiner {
    fn name(&self) -> &str {
        "YCombiner"
    }
    fn num_inputs(&self) -> usize {
        2
    }
    fn num_outputs(&self) -> usize {
        1
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        Matrix::full(1, 2, C64::new(0.5f64.sqrt(), 0.0))
    }
}

/// Waveguide crossing: input 0 exits on output 1 and vice versa.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Crossing;

impl Component for Crossing {
    fn name(&self) -> &str {
        "Crossing"
    }
    fn num_inputs(&self) -> usize {
        2
    }
    fn num_outputs(&self) -> usize {
        2
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        let (o, l) = (C64::new(0.0, 0.0), C64::new(1.0, 0.0));
        matrix(2, 2, &[o, l, l, o])
    }
}

/// Reflector with a pi phase shift on reflection.
#[derive(Clone, Debug, PartialEq)]
pub struct Mirror {
    /// Power reflectivity in `[0, 1]`.
    pub reflectivity: f64,
}

impl Mirror {
    pub fn new(reflectivity: f64) -> Self {
        Self { reflectivity }
    }

    pub fn perfect() -> Self {
        Self::new(1.0)
    }
}

impl Component for Mirror {
    fn name(&self) -> &str {
        "Mirror"
    }
    fn num_inputs(&self) -> usize {
        1
    }
    fn num_outputs(&self) -> usize {
        1
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        scalar(C64::new(-self.reflectivity.clamp(0.0, 1.0).sqrt(), 0.0))
    }
}

/// Monochromatic coherent source with a single output.
#[derive(Clone, Debug, PartialEq)]
pub struct Laser {
    /// Field amplitude; emitted power is `amplitude^2`.
    pub amplitude: f64,
    pub phase: f64,
    pub wavelength_nm: f64,
    /// Lasers sharing an id are phase-locked (see [`Component::source_id`]).
    pub source: u32,
}

impl Laser {
    pub fn new(amplitude: f64, wavelength_nm: f64) -> Self {
        Self {
            amplitude,
            phase: 0.0,
            wavelength_nm,
            source: 0,
        }
    }

    pub fn with_phase(mut self, phase: f64) -> Self {
        self.phase = phase;
        self
    }

    /// Put this laser in a coherence group: lasers with the same id interfere.
    pub fn with_source(mut self, source: u32) -> Self {
        self.source = source;
        self
    }
}

impl Component for Laser {
    fn name(&self) -> &str {
        "Laser"
    }
    fn num_inputs(&self) -> usize {
        0
    }
    fn num_outputs(&self) -> usize {
        1
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        Matrix::zeros(1, 0)
    }
    fn emission(&self, wavelength_nm: f64) -> Option<Vec<C64>> {
        ((wavelength_nm - self.wavelength_nm).abs() < 1e-9)
            .then(|| vec![C64::from_polar(self.amplitude, self.phase)])
    }
    fn emission_wavelengths(&self) -> Vec<f64> {
        vec![self.wavelength_nm]
    }
    fn source_id(&self) -> u32 {
        self.source
    }
}

/// Absorbs everything on its inputs. The power is read with
/// [`Solution::detected_power`](super::Solution::detected_power).
#[derive(Clone, Debug, PartialEq)]
pub struct Detector {
    pub inputs: usize,
}

impl Detector {
    pub fn new() -> Self {
        Self::with_inputs(1)
    }

    /// A detector with several inputs whose powers are summed incoherently.
    pub fn with_inputs(inputs: usize) -> Self {
        Self { inputs }
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Detector {
    fn name(&self) -> &str {
        "Detector"
    }
    fn num_inputs(&self) -> usize {
        self.inputs
    }
    fn num_outputs(&self) -> usize {
        0
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        Matrix::zeros(0, self.inputs)
    }
}

/// Wavelength-independent component with an arbitrary scattering matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct Custom {
    pub name: String,
    pub matrix: Matrix,
}

impl Custom {
    pub fn new(name: impl Into<String>, matrix: Matrix) -> Self {
        Self {
            name: name.into(),
            matrix,
        }
    }
}

impl Component for Custom {
    fn name(&self) -> &str {
        &self.name
    }
    fn num_inputs(&self) -> usize {
        self.matrix.ncols()
    }
    fn num_outputs(&self) -> usize {
        self.matrix.nrows()
    }
    fn s_matrix(&self, _wavelength_nm: f64) -> Matrix {
        self.matrix.clone()
    }
}

/// Scattering matrices at a set of wavelengths, ascending: what a measured
/// or separately simulated component is known by.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub wavelengths_nm: Vec<f64>,
    /// `outputs x inputs`, one per wavelength.
    pub matrices: Vec<Matrix>,
}

impl Table {
    /// The matrix at `wavelength_nm`: magnitude and phase interpolated
    /// linearly between the two nearest wavelengths (phase the short way
    /// round, so a resonance's phase doesn't jump); outside the table, the
    /// nearest end.
    pub fn at(&self, wavelength_nm: f64) -> Matrix {
        let w = &self.wavelengths_nm;
        let last = w.len().saturating_sub(1);
        if w.is_empty() {
            return Matrix::zeros(0, 0);
        }
        if wavelength_nm <= w[0] || last == 0 {
            return self.matrices[0].clone();
        }
        if wavelength_nm >= w[last] {
            return self.matrices[last].clone();
        }
        let k = w.partition_point(|&x| x <= wavelength_nm).clamp(1, last) - 1;
        let t = (wavelength_nm - w[k]) / (w[k + 1] - w[k]);
        let (a, b) = (&self.matrices[k], &self.matrices[k + 1]);
        Matrix::from_fn(a.nrows(), a.ncols(), |r, c| {
            let (x, y) = (a[(r, c)], b[(r, c)]);
            let mut turn = y.arg() - x.arg();
            if turn > PI {
                turn -= 2.0 * PI;
            } else if turn < -PI {
                turn += 2.0 * PI;
            }
            C64::from_polar(x.norm() + t * (y.norm() - x.norm()), x.arg() + t * turn)
        })
    }
}

/// A component defined by a [`Table`], shared rather than copied.
#[derive(Clone, Debug)]
pub struct Tabulated {
    pub name: String,
    pub table: std::sync::Arc<Table>,
}

impl Component for Tabulated {
    fn name(&self) -> &str {
        &self.name
    }
    fn num_inputs(&self) -> usize {
        self.table.matrices.first().map_or(0, |m| m.ncols())
    }
    fn num_outputs(&self) -> usize {
        self.table.matrices.first().map_or(0, |m| m.nrows())
    }
    fn s_matrix(&self, wavelength_nm: f64) -> Matrix {
        self.table.at(wavelength_nm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WL: f64 = 1550.0;

    fn close(a: C64, b: C64) -> bool {
        (a - b).norm() < 1e-12
    }

    fn assert_unitary(m: &Matrix) {
        let p = m.adjoint() * m;
        let id = Matrix::identity(m.ncols(), m.ncols());
        assert!((p - id).norm_l2() < 1e-12, "not unitary: {m:?}");
    }

    /// Shape reported by the trait must match the matrix it produces.
    fn assert_consistent(c: &dyn Component, name: &str, inputs: usize, outputs: usize) {
        assert_eq!(c.name(), name);
        assert_eq!((c.num_inputs(), c.num_outputs()), (inputs, outputs));
        let s = c.s_matrix(WL);
        assert_eq!((s.nrows(), s.ncols()), (outputs, inputs), "{name}");
    }

    #[test]
    fn port_counts_and_names() {
        assert_consistent(&Waveguide::new(1.0, 1.0), "Waveguide", 1, 1);
        assert_consistent(&PhaseShifter::new(0.0), "PhaseShifter", 1, 1);
        assert_consistent(&Attenuator::from_db(1.0), "Attenuator", 1, 1);
        assert_consistent(&DirectionalCoupler::balanced(), "DirectionalCoupler", 2, 2);
        assert_consistent(&Mzi::new(0.0, 0.0), "Mzi", 2, 2);
        assert_consistent(&YSplitter, "YSplitter", 1, 2);
        assert_consistent(&YCombiner, "YCombiner", 2, 1);
        assert_consistent(&Crossing, "Crossing", 2, 2);
        assert_consistent(&Mirror::perfect(), "Mirror", 1, 1);
        assert_consistent(&Laser::new(1.0, WL), "Laser", 0, 1);
        assert_consistent(&Detector::new(), "Detector", 1, 0);
        assert_consistent(&Detector::with_inputs(5), "Detector", 5, 0);
        assert_consistent(&Custom::new("X", Matrix::zeros(3, 4)), "X", 4, 3);
    }

    #[test]
    fn db_conversion() {
        assert_eq!(db_to_amplitude(0.0), 1.0);
        assert!((db_to_amplitude(20.0) - 0.1).abs() < 1e-15);
        // 10 dB is a tenth of the power.
        assert!((db_to_amplitude(10.0).powi(2) - 0.1).abs() < 1e-15);
    }

    #[test]
    fn propagation_phase_scales_with_index_and_length() {
        assert!((propagation_phase(1.55, 1.0, 1550.0) - 2.0 * PI).abs() < 1e-12);
        assert!((propagation_phase(1.55, 2.0, 1550.0) - 4.0 * PI).abs() < 1e-12);
        assert!((propagation_phase(0.775, 1.0, 1550.0) - PI).abs() < 1e-12);
        assert_eq!(propagation_phase(0.0, 3.0, 1550.0), 0.0);
    }

    #[test]
    fn matrix_helper_is_row_major() {
        let m = matrix(
            2,
            3,
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0].map(|v| C64::new(v, 0.0)),
        );
        assert_eq!(m[(0, 2)], C64::new(3.0, 0.0));
        assert_eq!(m[(1, 0)], C64::new(4.0, 0.0));
    }

    #[test]
    #[should_panic]
    fn matrix_helper_rejects_wrong_length() {
        matrix(2, 2, &[C64::ONE; 3]);
    }

    #[test]
    fn waveguide_loss_and_phase() {
        // 1 cm at 3 dB/cm.
        let wg = Waveguide::new(10_000.0, 1.5).with_loss(3.0);
        assert!((wg.transmission(WL).norm() - db_to_amplitude(3.0)).abs() < 1e-12);
        // One wavelength of optical path is a full turn.
        assert!(close(Waveguide::new(1.55, 1.0).transmission(WL), C64::ONE));
        // Half a wavelength flips the sign.
        assert!(close(
            Waveguide::free_space(0.775).transmission(WL),
            -C64::ONE
        ));
        let wg = Waveguide::free_space(3.0);
        assert_eq!(wg.n_eff, 1.0);
        assert_eq!(wg.loss_db_per_cm, 0.0);
        assert_eq!(wg.s_matrix(WL)[(0, 0)], wg.transmission(WL));
    }

    #[test]
    fn waveguide_is_dispersive() {
        let wg = Waveguide::new(100.0, 2.0);
        assert!(!close(wg.transmission(1550.0), wg.transmission(1551.0)));
    }

    #[test]
    fn group_index_sets_the_group_delay() {
        // The group delay is dφ/dω; with n_g it must come out as n_g·L/c,
        // whatever n_eff is.
        let wg = Waveguide::new(1000.0, 2.4).with_group_index(4.2, 1550.0);
        assert_eq!(wg.index_at(1550.0), 2.4, "n_eff holds at the reference");
        assert!(wg.index_at(1560.0) < 2.4, "normal dispersion: index falls with λ");
        let omega = |nm: f64| 2.0 * PI * SPEED_OF_LIGHT / (nm * 1e-9);
        let phase = |nm: f64| propagation_phase(wg.length_um, wg.index_at(nm), nm);
        let (a, b) = (1549.99, 1550.01);
        let tau_ps = (phase(a) - phase(b)) / (omega(a) - omega(b)) * 1e12;
        assert!((tau_ps - wg.group_delay_ps()).abs() / wg.group_delay_ps() < 1e-6, "{tau_ps} vs {}", wg.group_delay_ps());
        assert!((wg.group_delay_ps() - 4.2 * 1e-3 / SPEED_OF_LIGHT * 1e12).abs() < 1e-9, "1 mm at n_g 4.2 ≈ 14 ps");
        // Without a group index, nothing changes from a constant index.
        let plain = Waveguide::new(1000.0, 2.4);
        for nm in [1500.0, 1550.0, 1600.0] {
            assert!(close(plain.transmission(nm), C64::from_polar(1.0, propagation_phase(1000.0, 2.4, nm))));
        }
        assert_eq!(effective_index(2.4, 2.4, 1550.0, 1300.0), 2.4);
    }

    #[test]
    fn phase_shifter_is_wavelength_independent() {
        let ps = PhaseShifter::new(PI / 2.0);
        assert!(close(ps.s_matrix(500.0)[(0, 0)], C64::new(0.0, 1.0)));
        assert!(close(ps.s_matrix(1550.0)[(0, 0)], C64::new(0.0, 1.0)));
    }

    #[test]
    fn attenuator_constructors_agree() {
        let a = Attenuator::from_db(3.0).amplitude;
        let b = Attenuator::from_power_transmission(db_to_amplitude(3.0).powi(2)).amplitude;
        assert!((a - b).abs() < 1e-12);
        assert_eq!(
            Attenuator::from_amplitude(0.25).s_matrix(WL)[(0, 0)],
            C64::new(0.25, 0.0)
        );
        // Out-of-range power transmissions are clamped.
        assert_eq!(Attenuator::from_power_transmission(2.0).amplitude, 1.0);
        assert_eq!(Attenuator::from_power_transmission(-1.0).amplitude, 0.0);
    }

    #[test]
    fn coupler_split_ratio_and_phase() {
        let s = DirectionalCoupler::new(0.3).s_matrix(WL);
        assert!((s[(0, 0)].norm_sqr() - 0.7).abs() < 1e-12);
        assert!((s[(1, 0)].norm_sqr() - 0.3).abs() < 1e-12);
        // Cross port lags by pi/2.
        assert!((s[(1, 0)].arg() - PI / 2.0).abs() < 1e-12);
        assert_eq!(s[(0, 1)], s[(1, 0)], "reciprocal");
        // Extremes, including clamping.
        assert!(close(
            DirectionalCoupler::new(0.0).s_matrix(WL)[(1, 0)],
            C64::ZERO
        ));
        assert!(close(
            DirectionalCoupler::new(1.0).s_matrix(WL)[(0, 0)],
            C64::ZERO
        ));
        assert_eq!(
            DirectionalCoupler::new(1.5).s_matrix(WL),
            DirectionalCoupler::new(1.0).s_matrix(WL)
        );
        assert_eq!(DirectionalCoupler::balanced().coupling, 0.5);
        let _: BeamSplitter = DirectionalCoupler::balanced();
    }

    #[test]
    fn lossless_components_are_unitary() {
        assert_unitary(&DirectionalCoupler::new(0.3).s_matrix(WL));
        assert_unitary(&Mzi::new(0.7, 2.1).s_matrix(WL));
        assert_unitary(&Crossing.s_matrix(WL));
        assert_unitary(&PhaseShifter::new(1.0).s_matrix(WL));
        assert_unitary(&Waveguide::new(12.3, 2.4).s_matrix(WL));
        assert_unitary(&Mirror::perfect().s_matrix(WL));
    }

    #[test]
    fn mzi_bar_and_cross_states() {
        let cross = Mzi::new(0.0, 0.0).s_matrix(WL);
        assert!(cross[(0, 0)].norm() < 1e-12 && (cross[(1, 0)].norm() - 1.0).abs() < 1e-12);
        let bar = Mzi::new(PI, 0.0).s_matrix(WL);
        assert!(bar[(1, 0)].norm() < 1e-12 && (bar[(0, 0)].norm() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn mzi_equals_coupler_phase_coupler_composition() {
        let dc = DirectionalCoupler::balanced().s_matrix(WL);
        for (theta, phi) in [(0.3, 1.1), (2.0, -0.4), (PI, PI)] {
            let inner = matrix(
                2,
                2,
                &[C64::from_polar(1.0, theta), C64::ZERO, C64::ZERO, C64::ONE],
            );
            let outer = matrix(
                2,
                2,
                &[C64::from_polar(1.0, phi), C64::ZERO, C64::ZERO, C64::ONE],
            );
            let composed = &dc * &inner * &dc * &outer;
            assert!((composed - Mzi::matrix(theta, phi)).norm_l2() < 1e-12);
            assert_eq!(Mzi::new(theta, phi).s_matrix(WL), Mzi::matrix(theta, phi));
        }
    }

    #[test]
    fn y_junctions() {
        let split = YSplitter.s_matrix(WL);
        assert!((split[(0, 0)].norm_sqr() + split[(1, 0)].norm_sqr() - 1.0).abs() < 1e-12);
        let comb = YCombiner.s_matrix(WL);
        let v = 0.5f64.sqrt();
        // In-phase inputs combine fully, anti-phase inputs radiate away.
        let in_phase = comb[(0, 0)] * v + comb[(0, 1)] * v;
        let anti = comb[(0, 0)] * v - comb[(0, 1)] * v;
        assert!((in_phase.norm_sqr() - 1.0).abs() < 1e-12);
        assert!(anti.norm() < 1e-12);
    }

    #[test]
    fn crossing_swaps() {
        let s = Crossing.s_matrix(WL);
        assert_eq!(s[(1, 0)], C64::ONE);
        assert_eq!(s[(0, 1)], C64::ONE);
        assert_eq!(s[(0, 0)], C64::ZERO);
    }

    #[test]
    fn mirror_reflects_with_pi_phase() {
        assert!(close(Mirror::perfect().s_matrix(WL)[(0, 0)], -C64::ONE));
        assert!(close(
            Mirror::new(0.25).s_matrix(WL)[(0, 0)],
            C64::new(-0.5, 0.0)
        ));
        assert!(
            close(Mirror::new(4.0).s_matrix(WL)[(0, 0)], -C64::ONE),
            "clamped"
        );
    }

    #[test]
    fn laser_emits_only_at_its_wavelength() {
        let laser = Laser::new(2.0, 650.0).with_phase(PI / 2.0);
        let e = laser.emission(650.0).unwrap();
        assert_eq!(e.len(), 1);
        assert!(close(e[0], C64::new(0.0, 2.0)));
        assert!(laser.emission(651.0).is_none());
        assert_eq!(laser.emission_wavelengths(), vec![650.0]);
        assert_eq!(Laser::new(1.0, 1.0).phase, 0.0);
    }

    #[test]
    fn passive_components_do_not_emit() {
        let parts: [&dyn Component; 4] = [
            &YSplitter,
            &Detector::new(),
            &Mirror::perfect(),
            &Custom::new("c", Matrix::zeros(1, 1)),
        ];
        for p in parts {
            assert!(p.emission(WL).is_none());
            assert!(p.emission_wavelengths().is_empty());
        }
    }

    #[test]
    fn detector_default_is_single_input() {
        assert_eq!(Detector::default(), Detector::new());
        assert_eq!(Detector::new().inputs, 1);
    }

    #[test]
    fn custom_returns_its_matrix() {
        let m = matrix(1, 2, &[C64::new(1.0, 2.0), C64::new(3.0, 4.0)]);
        let c = Custom::new(String::from("thing"), m.clone());
        assert_eq!(c.s_matrix(1.0), m);
        assert_eq!(c.s_matrix(9999.0), m);
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;

    #[test]
    fn tables_interpolate_magnitude_and_phase() {
        let m = |v: C64| Matrix::from_fn(1, 1, |_, _| v);
        let t = Table {
            wavelengths_nm: vec![1500.0, 1510.0, 1520.0],
            matrices: vec![m(C64::from_polar(1.0, 3.0)), m(C64::from_polar(0.5, -3.0)), m(C64::new(0.2, 0.0))],
        };
        // At a table point: exactly it.
        assert!((t.at(1510.0)[(0, 0)] - C64::from_polar(0.5, -3.0)).norm() < 1e-12);
        // Halfway: magnitude 0.75, phase the short way across ±π (π, not 0).
        let mid = t.at(1505.0)[(0, 0)];
        assert!((mid.norm() - 0.75).abs() < 1e-12);
        assert!((mid.arg().abs() - PI).abs() < 1e-9, "{}", mid.arg());
        // Outside: the nearest end.
        assert_eq!(t.at(1400.0), t.matrices[0]);
        assert_eq!(t.at(1600.0), t.matrices[2]);
        let c = Tabulated { name: "measured".into(), table: std::sync::Arc::new(t) };
        assert_eq!((c.num_inputs(), c.num_outputs(), c.name()), (1, 1, "measured"));
    }
}
