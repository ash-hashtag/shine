//! Frequency-domain simulation of linear optical circuits.
//!
//! Every component is described by a scattering matrix that maps the complex
//! field amplitudes on its input ports to the amplitudes on its output ports
//! (`outputs x inputs`). Components are wired together in a [`Circuit`], which
//! is solved exactly as a linear system, so feedback paths (ring resonators,
//! cavities) are handled without any iteration limit.
//!
//! Conventions:
//! - fields are complex amplitudes normalised so that `|E|^2` is power,
//! - wavelengths are in nanometres, physical lengths in micrometres,
//! - a lossless 2x2 coupler is `[[t, i k], [i k, t]]` with `t^2 + k^2 = 1`.
//!
//! ```
//! use shine::photonics::*;
//!
//! // A Mach-Zehnder interferometer built from two 50:50 couplers.
//! let mut c = Circuit::new();
//! let laser = c.add(Laser::new(1.0, 1550.0));
//! let bs1 = c.add(DirectionalCoupler::balanced());
//! let ps = c.add(PhaseShifter::new(0.0));
//! let bs2 = c.add(DirectionalCoupler::balanced());
//! let d0 = c.add(Detector::new());
//! let d1 = c.add(Detector::new());
//! c.connect(laser.output(0), bs1.input(0)).unwrap();
//! c.connect(bs1.output(0), ps.input(0)).unwrap();
//! c.connect(ps.output(0), bs2.input(0)).unwrap();
//! c.connect(bs1.output(1), bs2.input(1)).unwrap();
//! c.connect(bs2.output(0), d0.input(0)).unwrap();
//! c.connect(bs2.output(1), d1.input(0)).unwrap();
//!
//! // Balanced arms: all light leaves on the cross port.
//! let sol = c.simulate(1550.0, &[]).unwrap();
//! assert!(sol.detected_power(d0) < 1e-12);
//! assert!((sol.detected_power(d1) - 1.0).abs() < 1e-12);
//!
//! // Compile once, then sweep the heater cheaply.
//! c.replace(ps, PhaseShifter::new(std::f64::consts::PI)).unwrap();
//! let sol = c.compile(1550.0).unwrap().run(&[]).unwrap();
//! assert!((sol.detected_power(d0) - 1.0).abs() < 1e-12);
//! ```
//!
//! Architecture: [`Circuit`] is the editable netlist; [`Circuit::compile`]
//! lowers it to a [`Program`] (flat slot-addressed multiply-add rows plus
//! pre-factored LU blocks for feedback loops), which runs batches of inputs
//! with SIMD kernels. Reusable sub-circuits implement [`Block`].

pub mod blocks;
pub mod circuit;
pub mod components;
pub mod mesh;
pub mod par;
pub mod program;
mod simd;

pub use circuit::{Block, Circuit, CircuitError, InPort, NodeId, OutPort, Ports};
pub use components::*;
pub use mesh::{MeshError, MziSetting, SvdMesh, UnitaryMesh};
pub use program::{Program, Solution};

pub use faer;
pub use num_complex::Complex64 as C64;

/// Dense complex matrix used for scattering matrices.
pub type Matrix = faer::Mat<C64>;
