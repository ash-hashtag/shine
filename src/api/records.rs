//! Plain data the API hands out and takes: ports, parameters, complex
//! numbers and matrices, readings.

use crate::photonics::{C64, Matrix as CoreMatrix};

/// Whether a port carries light (a waveguide) or a signal (a wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortKind {
    Light,
    Signal,
}

/// Whether light or a signal goes into a port or comes out of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    In,
    Out,
}

/// A port of a part, as [`crate::api::Part::ports`] lists them.
#[derive(Clone, Debug, PartialEq)]
pub struct PortInfo {
    pub name: String,
    pub kind: PortKind,
    pub direction: Direction,
}

/// A parameter of a part: its name, unit, current value and the range it
/// typically takes (values outside it are allowed).
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub unit: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

/// A complex number: a field's amplitude and phase as real and imaginary parts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// |z|²: the power of a field.
    pub fn norm_sqr(&self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    /// The angle, in radians.
    pub fn arg(&self) -> f64 {
        self.im.atan2(self.re)
    }
}

impl From<C64> for Complex {
    fn from(z: C64) -> Self {
        Self { re: z.re, im: z.im }
    }
}

impl From<Complex> for C64 {
    fn from(z: Complex) -> Self {
        C64::new(z.re, z.im)
    }
}

/// A matrix of complex numbers, row by row: element (row, column) is
/// `values[row * cols + column]`. For a transmission, rows are outputs and
/// columns inputs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Matrix {
    pub rows: u32,
    pub cols: u32,
    pub values: Vec<Complex>,
}

impl Matrix {
    /// A matrix from its rows.
    pub fn from_rows(rows: &[Vec<Complex>]) -> Self {
        let cols = rows.first().map_or(0, Vec::len);
        Self { rows: rows.len() as u32, cols: cols as u32, values: rows.iter().flat_map(|r| r.iter().copied()).collect() }
    }

    pub fn get(&self, row: u32, col: u32) -> Complex {
        self.values.get((row * self.cols + col) as usize).copied().unwrap_or_default()
    }

    pub(crate) fn from_core(m: &CoreMatrix) -> Self {
        Self {
            rows: m.nrows() as u32,
            cols: m.ncols() as u32,
            values: (0..m.nrows()).flat_map(|r| (0..m.ncols()).map(move |c| m[(r, c)].into())).collect(),
        }
    }
}

/// The power arriving at a detector or a design's light output.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub name: String,
    pub power_mw: f64,
}

/// One curve of a sweep: an output's power at every swept value.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub name: String,
    pub power_mw: Vec<f64>,
}

/// What a wavelength sweep says about an output's resonances.
#[derive(Clone, Debug, PartialEq)]
pub struct Readouts {
    /// Whether the resonances are dips (else peaks).
    pub dips: bool,
    /// Where each resonance is, in nm.
    pub resonances_nm: Vec<f64>,
    /// Free spectral range: the mean spacing of neighbouring resonances.
    pub fsr_nm: Option<f64>,
    /// Full width at half depth of the strongest resonance.
    pub fwhm_nm: Option<f64>,
    /// Quality factor: wavelength over width.
    pub q: Option<f64>,
    /// Largest over smallest power, in dB.
    pub extinction_db: f64,
}

/// Mean, spread and extremes of an output over Monte Carlo runs, in mW.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub mean: f64,
    pub std: f64,
    pub min: f64,
    pub max: f64,
}

/// How much each kind of parameter varies in a Monte Carlo run (standard
/// deviations): effective index (absolute), coupling (relative, 0.03 = 3%),
/// phase (radians) and loss (relative).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variation {
    pub n_eff: f64,
    pub coupling: f64,
    pub phase: f64,
    pub loss: f64,
}

impl Default for Variation {
    /// Typical of silicon photonics, as in the app.
    fn default() -> Self {
        let v = crate::panels::circuit::monte_carlo::Variation::default();
        Self { n_eff: v.n_eff, coupling: v.coupling, phase: v.phase, loss: v.loss }
    }
}

/// What an optimisation aims for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Goal {
    Maximise,
    Minimise,
    /// Reach this power, in mW.
    Target(f64),
}

/// What an optimisation found.
#[derive(Clone, Debug, PartialEq)]
pub struct Optimised {
    /// The best values, one per variable, in the order given.
    pub values: Vec<f64>,
    /// The output's power before and after, in mW.
    pub before_mw: f64,
    pub after_mw: f64,
    pub simulations: u32,
}
