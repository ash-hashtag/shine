//! Shine as a library: build components and designs, simulate them, sweep,
//! analyse, optimise and export, without the app. What it builds is a
//! normal Shine project: save it and open it in the app to look at it, or
//! open a file made in the app and script it.
//!
//! The Python and TypeScript packages (`shine-photonics`) are this API.
//!
//! ```
//! use shine::api::Project;
//!
//! let project = Project::new();
//! let chip = project.new_design("Ring filter");
//! let laser = chip.laser(1550.0, 1.0)?;            // wavelength_nm, power_mw
//! let ring = chip.ring(0.05, 100.0)?;              // coupling, length_um
//! let detector = chip.detector();
//! chip.chain(&[laser.clone(), ring.clone(), detector.clone()])?;
//!
//! let power = chip.simulate()?.power_mw(&detector)?;
//! assert!(power > 0.0 && power <= 1.0);
//!
//! let spectrum = chip.sweep(&laser, "wavelength", 1540.0, 1560.0, 2001)?;
//! let readouts = spectrum.readouts("Detector")?;   // resonances, FSR, width, Q
//! assert!(readouts.dips && readouts.fsr_nm.is_some());
//! # Ok::<(), shine::api::Error>(())
//! ```
//!
//! Conventions, as in the app: fields are complex amplitudes with |E|² in
//! mW; wavelengths in nm, lengths in µm, phases in radians, loss in dB or
//! dB/cm; light runs one way, from outputs to inputs. Parts, ports and
//! parameters have the names the app's inspector and sweeps use.

mod analysis;
mod arrange;
mod design;
mod error;
mod part;
mod project;
mod records;

pub use analysis::{MonteCarlo, Simulation, Sweep, Transmission};
pub use design::Design;
pub use error::{Error, Result};
pub use part::{Part, Port};
pub use project::{Project, TransferFunction, layer_scale};
pub use records::{Aim, Aimed, Complex, Direction, Goal, Matrix, Optimised, Param, PortInfo, PortKind, Reading, Readouts, Series, Stats, Variation};

#[cfg(test)]
mod tests;
