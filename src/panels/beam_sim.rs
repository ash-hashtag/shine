//! Compiles a free-space grid layout into a [`photonics::Circuit`] and solves it.
//!
//! Every placed component becomes a node with one port per propagation
//! direction (8 directions, 45 degrees apart): input `k` receives light
//! travelling in direction `k`, output `k` emits light travelling in direction
//! `k`. Beams between components are free-space [`Waveguide`]s whose length
//! comes from the grid, so propagation phase and interference are exact, and
//! feedback (cavities) is solved in closed form by the circuit solver.

use std::f32::consts::FRAC_PI_4;

use egui::{Pos2, Vec2};

use super::circuit::sim;
use super::optics::model::Layout;
use super::optics::{OpticComponentKind, PlacedComponent};
use crate::photonics::{C64, Matrix};

pub const DIRECTIONS: usize = 8;

pub fn direction(k: usize) -> Vec2 {
    let a = k as f32 * FRAC_PI_4;
    egui::vec2(a.cos(), a.sin())
}

pub fn direction_index(v: Vec2) -> usize {
    ((v.y.atan2(v.x) / FRAC_PI_4).round() as i32).rem_euclid(DIRECTIONS as i32) as usize
}

/// Field on one straight beam segment at one wavelength.
#[derive(Clone, Debug, PartialEq)]
pub struct Beam {
    pub start: Pos2,
    pub end: Pos2,
    pub dir: Vec2,
    pub wavelength_nm: f64,
    /// Field at the start of the segment.
    pub field: C64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutSimulation {
    pub beams: Vec<Beam>,
    /// `(component index, power summed over all wavelengths)`.
    pub detector_power: Vec<(usize, f64)>,
    pub error: Option<String>,
}

/// How a component scatters light between the 8 directions, or `None` for
/// components that are not passive scatterers (sources, detectors).
pub fn scattering(kind: &OpticComponentKind) -> Option<Matrix> {
    let mut s = Matrix::zeros(DIRECTIONS, DIRECTIONS);
    let reflect = |s: &mut Matrix, normal: Vec2, r: C64, t: C64| {
        let n = normal.normalized();
        for k in 0..DIRECTIONS {
            let d = direction(k);
            let dot = d.dot(n);
            if dot.abs() < 1e-3 {
                // Grazing: the beam runs along the surface and is unaffected.
                s[(k, k)] = C64::ONE;
                continue;
            }
            s[(direction_index(d - 2.0 * dot * n), k)] += r;
            s[(k, k)] += t;
        }
    };

    match *kind {
        OpticComponentKind::Mirror { normal } => {
            // Reflect from the front, absorb on the back.
            let n = normal.normalized();
            for k in 0..DIRECTIONS {
                let d = direction(k);
                let dot = d.dot(n);
                if dot.abs() < 1e-3 {
                    s[(k, k)] = C64::ONE;
                } else if dot < 0.0 {
                    s[(direction_index(d - 2.0 * dot * n), k)] = -C64::ONE;
                }
            }
        }
        OpticComponentKind::BeamSplitter {
            reflectivity,
            transparency,
            normal,
        } => reflect(
            &mut s,
            normal,
            C64::new(0.0, (reflectivity.clamp(0.0, 1.0) as f64).sqrt()),
            C64::new((transparency.clamp(0.0, 1.0) as f64).sqrt(), 0.0),
        ),
        OpticComponentKind::PhaseShifter { phase } => {
            for k in 0..DIRECTIONS {
                s[(k, k)] = C64::from_polar(1.0, phase as f64);
            }
        }
        OpticComponentKind::Attenuator { transmission } => {
            for k in 0..DIRECTIONS {
                s[(k, k)] = C64::new((transmission.clamp(0.0, 1.0) as f64).sqrt(), 0.0);
            }
        }
        // Thin lens on axis: no deflection in a ray-per-cell model.
        OpticComponentKind::Lens => {
            for k in 0..DIRECTIONS {
                s[(k, k)] = C64::ONE;
            }
        }
        OpticComponentKind::Laser { .. }
        | OpticComponentKind::Detector { .. }
        | OpticComponentKind::InputPort { .. }
        | OpticComponentKind::OutputPort
        | OpticComponentKind::SignalIn
        | OpticComponentKind::SignalOut => return None,
    }
    Some(s)
}

/// Simulate a layout made only of primitives. `cell_um` is the physical size
/// of one grid cell; beams leaving the layout are drawn `max_len` long.
/// Layouts with boxes and signals use [`crate::panels::circuit::sim::simulate`].
pub fn simulate_layout(components: &[PlacedComponent], _grid_size: f32, cell_um: f64, max_len: f32) -> LayoutSimulation {
    let layout = Layout::with_components(components.to_vec());
    let outcome = sim::simulate(
        &layout,
        cell_um,
        &sim::NoDocuments,
        &sim::Machine::default(),
        None,
        &sim::Options {
            max_len,
            ..sim::Options::default()
        },
    );
    LayoutSimulation {
        beams: outcome.beams,
        detector_power: outcome.detector_power,
        error: outcome.error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, kind: OpticComponentKind) -> PlacedComponent {
        PlacedComponent {
            kind,
            pos: Pos2::new(x * 40.0, y * 40.0),
        }
    }

    fn splitter(nx: f32, ny: f32) -> OpticComponentKind {
        OpticComponentKind::BeamSplitter {
            reflectivity: 0.5,
            transparency: 0.5,
            normal: egui::vec2(nx, ny).normalized(),
        }
    }

    fn mirror(nx: f32, ny: f32) -> OpticComponentKind {
        OpticComponentKind::Mirror {
            normal: egui::vec2(nx, ny).normalized(),
        }
    }

    /// Mach-Zehnder on the grid: BS(0,0) -> mirrors (2,0) and (0,2) -> BS(2,2).
    fn mzi(phase: f32) -> Vec<PlacedComponent> {
        vec![
            at(
                -2.0,
                0.0,
                OpticComponentKind::Laser {
                    amplitude: 1.0,
                    phase: 0.0,
                    wavelength: 650.0,
                    dir: egui::vec2(1.0, 0.0),
                },
            ),
            at(0.0, 0.0, splitter(-1.0, 1.0)),
            at(1.0, 0.0, OpticComponentKind::PhaseShifter { phase }),
            at(2.0, 0.0, mirror(-1.0, 1.0)),
            at(0.0, 2.0, mirror(1.0, -1.0)),
            at(2.0, 2.0, splitter(-1.0, 1.0)),
            at(4.0, 2.0, crate::panels::optics::model::DETECTOR),
            at(2.0, 4.0, crate::panels::optics::model::DETECTOR),
        ]
    }

    fn powers(sim: &LayoutSimulation) -> (f64, f64) {
        let p = |i| sim.detector_power.iter().find(|d| d.0 == i).unwrap().1;
        (p(6), p(7))
    }

    #[test]
    fn grid_mzi_interferes() {
        let (a, b) = powers(&simulate_layout(&mzi(0.0), 40.0, 10.0, 2000.0));
        assert!((a + b - 1.0).abs() < 1e-9, "energy conserved: {a} + {b}");
        let (c, d) = powers(&simulate_layout(
            &mzi(std::f32::consts::PI),
            40.0,
            10.0,
            2000.0,
        ));
        // A pi phase in one arm swaps the output ports.
        assert!((a - d).abs() < 1e-5 && (b - c).abs() < 1e-5);
        assert!(
            (a - b).abs() > 0.99,
            "fully constructive on one side: {a}, {b}"
        );
    }

    #[test]
    fn directions_round_trip() {
        for k in 0..DIRECTIONS {
            assert_eq!(direction_index(direction(k)), k);
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    const GRID: f32 = 40.0;
    const FAR: f32 = 2000.0;

    fn at(x: f32, y: f32, kind: OpticComponentKind) -> PlacedComponent {
        PlacedComponent {
            kind,
            pos: Pos2::new(x * GRID, y * GRID),
        }
    }

    fn laser(dir: Vec2, wavelength: f32) -> OpticComponentKind {
        OpticComponentKind::Laser {
            amplitude: 1.0,
            phase: 0.0,
            wavelength,
            dir,
        }
    }

    fn right() -> Vec2 {
        egui::vec2(1.0, 0.0)
    }

    fn power(sim: &LayoutSimulation, index: usize) -> f64 {
        sim.detector_power.iter().find(|d| d.0 == index).unwrap().1
    }

    fn is_unitary(m: &Matrix) -> bool {
        (m.adjoint() * m - Matrix::identity(m.ncols(), m.ncols())).norm_l2() < 1e-6
    }

    #[test]
    fn mirror_scattering() {
        let s = scattering(&OpticComponentKind::Mirror {
            normal: egui::vec2(-1.0, -1.0).normalized(),
        })
        .unwrap();
        // Rightwards light is turned upwards (direction 6) with a pi phase.
        assert_eq!(s[(6, 0)], -C64::ONE);
        // Leftwards light hits the back and is absorbed.
        assert!((0..DIRECTIONS).all(|j| s[(j, 4)] == C64::ZERO));
        // Light running along the surface (direction 7) is untouched.
        assert_eq!(s[(7, 7)], C64::ONE);
    }

    #[test]
    fn lossless_splitter_is_unitary() {
        let s = scattering(&OpticComponentKind::BeamSplitter {
            reflectivity: 0.3,
            transparency: 0.7,
            normal: egui::vec2(-1.0, -1.0).normalized(),
        })
        .unwrap();
        assert!(is_unitary(&s));
        assert!((s[(6, 0)] - C64::new(0.0, 0.3f64.sqrt())).norm() < 1e-6);
        assert!((s[(0, 0)] - C64::new(0.7f64.sqrt(), 0.0)).norm() < 1e-6);
        // Both faces reflect: leftwards light turns downwards.
        assert!(s[(2, 4)].norm() > 0.5);
    }

    #[test]
    fn inline_elements_scatter_diagonally() {
        let ps = scattering(&OpticComponentKind::PhaseShifter { phase: 1.0 }).unwrap();
        let att = scattering(&OpticComponentKind::Attenuator { transmission: 0.25 }).unwrap();
        let lens = scattering(&OpticComponentKind::Lens).unwrap();
        for i in 0..DIRECTIONS {
            for j in 0..DIRECTIONS {
                let diag = i == j;
                assert_eq!(ps[(i, j)] != C64::ZERO, diag);
                assert_eq!(att[(i, j)] != C64::ZERO, diag);
                assert_eq!(lens[(i, j)], if diag { C64::ONE } else { C64::ZERO });
            }
            assert!((ps[(i, i)] - C64::from_polar(1.0, 1.0)).norm() < 1e-6);
            assert_eq!(att[(i, i)], C64::new(0.5, 0.0));
        }
        assert!(scattering(&crate::panels::optics::model::DETECTOR).is_none());
        assert!(scattering(&laser(right(), 650.0)).is_none());
    }

    #[test]
    fn direction_index_snaps_and_wraps() {
        assert_eq!(direction_index(egui::vec2(1.0, 0.1)), 0);
        assert_eq!(direction_index(egui::vec2(0.0, -1.0)), 6);
        assert_eq!(direction_index(egui::vec2(1.0, -0.01)), 0);
        assert_eq!(direction_index(egui::vec2(-1.0, 0.0)), 4);
    }

    #[test]
    fn empty_layout() {
        let sim = simulate_layout(&[], GRID, 10.0, FAR);
        assert!(sim.beams.is_empty() && sim.detector_power.is_empty() && sim.error.is_none());
    }

    #[test]
    fn lone_laser_escapes() {
        let mut kind = laser(right(), 532.0);
        if let OpticComponentKind::Laser {
            amplitude, phase, ..
        } = &mut kind
        {
            *amplitude = 2.0;
            *phase = 1.0;
        }
        let sim = simulate_layout(&[at(0.0, 0.0, kind)], GRID, 10.0, FAR);
        assert_eq!(sim.beams.len(), 1);
        let beam = &sim.beams[0];
        assert_eq!(beam.wavelength_nm, 532.0);
        assert_eq!(beam.end, Pos2::new(FAR, 0.0));
        assert_eq!(beam.dir, direction(0));
        assert!((beam.field - C64::from_polar(2.0, 1.0)).norm() < 1e-6);
    }

    #[test]
    fn detector_reads_laser_through_inline_elements() {
        for (middle, expected) in [
            (OpticComponentKind::Lens, 1.0),
            (OpticComponentKind::PhaseShifter { phase: 2.0 }, 1.0),
            (OpticComponentKind::Attenuator { transmission: 0.25 }, 0.25),
        ] {
            let layout = [
                at(0.0, 0.0, laser(right(), 650.0)),
                at(2.0, 0.0, middle),
                at(5.0, 0.0, crate::panels::optics::model::DETECTOR),
            ];
            let sim = simulate_layout(&layout, GRID, 10.0, FAR);
            assert!((power(&sim, 2) - expected).abs() < 1e-9, "{middle:?}");
            assert_eq!(sim.beams.len(), 2);
            assert!(sim.beams.iter().any(|b| b.end == layout[2].pos));
        }
    }

    #[test]
    fn propagation_phase_follows_cell_size() {
        // Half-wave cells: each cell of travel flips the sign.
        let layout = [
            at(0.0, 0.0, laser(right(), 650.0)),
            at(1.0, 0.0, OpticComponentKind::PhaseShifter { phase: 0.0 }),
            at(3.0, 0.0, crate::panels::optics::model::DETECTOR),
        ];
        let sim = simulate_layout(&layout, GRID, 0.325, FAR);
        let from_shifter = sim.beams.iter().find(|b| b.start == layout[1].pos).unwrap();
        assert!((from_shifter.field - -C64::ONE).norm() < 1e-9);
    }

    #[test]
    fn blocked_paths_terminate() {
        let layout = [
            at(0.0, 0.0, laser(right(), 650.0)),
            // Back of the mirror faces the laser.
            at(
                2.0,
                0.0,
                OpticComponentKind::Mirror {
                    normal: egui::vec2(1.0, 0.0),
                },
            ),
            at(4.0, 0.0, crate::panels::optics::model::DETECTOR),
        ];
        let sim = simulate_layout(&layout, GRID, 10.0, FAR);
        assert_eq!(power(&sim, 2), 0.0);
        assert_eq!(sim.beams.len(), 1);
        assert_eq!(sim.beams[0].end, layout[1].pos);
    }

    #[test]
    fn lasers_absorb_incoming_light() {
        let layout = [
            at(0.0, 0.0, laser(right(), 650.0)),
            at(3.0, 0.0, laser(egui::vec2(-1.0, 0.0), 650.0)),
        ];
        let sim = simulate_layout(&layout, GRID, 10.0, FAR);
        assert!(sim.error.is_none());
        assert_eq!(sim.beams.len(), 2);
        assert!(
            sim.beams
                .iter()
                .all(|b| b.end == layout[0].pos || b.end == layout[1].pos)
        );
    }

    #[test]
    fn diagonal_beams_connect() {
        let layout = [
            at(0.0, 0.0, laser(egui::vec2(1.0, 1.0).normalized(), 650.0)),
            at(3.0, 3.0, crate::panels::optics::model::DETECTOR),
            // Off the diagonal: must not be hit.
            at(3.0, 2.0, crate::panels::optics::model::DETECTOR),
        ];
        let sim = simulate_layout(&layout, GRID, 10.0, FAR);
        assert!((power(&sim, 1) - 1.0).abs() < 1e-9);
        assert_eq!(power(&sim, 2), 0.0);
    }

    #[test]
    fn wavelengths_add_incoherently() {
        let layout = [
            at(0.0, 0.0, laser(right(), 650.0)),
            at(4.0, 0.0, laser(egui::vec2(-1.0, 0.0), 532.0)),
            at(2.0, 0.0, crate::panels::optics::model::DETECTOR),
        ];
        let sim = simulate_layout(&layout, GRID, 10.0, FAR);
        assert!((power(&sim, 2) - 2.0).abs() < 1e-9);
        let mut wls: Vec<f64> = sim.beams.iter().map(|b| b.wavelength_nm).collect();
        wls.sort_by(f64::total_cmp);
        assert_eq!(wls, vec![532.0, 650.0]);
    }

    #[test]
    fn resonant_lossless_cavity_reports_error() {
        let wall = |nx: f32| OpticComponentKind::BeamSplitter {
            reflectivity: 1.0,
            transparency: 0.0,
            normal: egui::vec2(nx, 0.0),
        };
        // Round trip: 6 cells of travel plus two i-reflections (= pi).
        // Pick the cell so the travel adds another pi: resonance.
        let cell_um = 0.65 * 0.5 / 6.0;
        let layout = [
            at(0.0, 0.0, wall(1.0)),
            at(3.0, 0.0, wall(-1.0)),
            at(0.0, 5.0, laser(right(), 650.0)),
        ];
        let sim = simulate_layout(&layout, GRID, cell_um, FAR);
        assert!(
            sim.error
                .as_deref()
                .unwrap()
                .contains("no unique steady state")
        );
        assert!(sim.beams.is_empty());
        // Off resonance the same cavity is fine.
        assert!(
            simulate_layout(&layout, GRID, cell_um * 1.1, FAR)
                .error
                .is_none()
        );
    }
}

#[cfg(test)]
mod zero_tests {
    use super::*;

    #[test]
    fn dark_readings_are_positive_zero() {
        // No laser, so there are no wavelengths to sum over.
        let parts = [
            PlacedComponent {
                kind: crate::panels::optics::model::DETECTOR,
                pos: Pos2::new(20.0, 20.0),
            },
            PlacedComponent {
                kind: OpticComponentKind::OutputPort,
                pos: Pos2::new(60.0, 20.0),
            },
        ];
        let sim = simulate_layout(&parts, 40.0, 10.0, 1000.0);
        for (_, power) in sim.detector_power {
            assert!(power == 0.0 && power.is_sign_positive(), "{power}");
            assert_eq!(format!("{power:.3}"), "0.000");
        }
    }
}
