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

use super::optics::{GRID, OpticComponentKind, PlacedComponent};
use crate::photonics::{
    Block, C64, Circuit, CircuitError, Custom, Detector, InPort, Laser, Matrix, NodeId, OutPort,
    PhaseShifter, Ports, Waveguide,
};

pub const DIRECTIONS: usize = 8;

pub fn direction(k: usize) -> Vec2 {
    let a = k as f32 * FRAC_PI_4;
    egui::vec2(a.cos(), a.sin())
}

pub fn direction_index(v: Vec2) -> usize {
    ((v.y.atan2(v.x) / FRAC_PI_4).round() as i32).rem_euclid(DIRECTIONS as i32) as usize
}

/// Field on one straight beam segment at one wavelength.
pub struct Beam {
    pub start: Pos2,
    pub end: Pos2,
    pub dir: Vec2,
    pub wavelength_nm: f64,
    /// Field at the start of the segment.
    pub field: C64,
}

#[derive(Default)]
pub struct LayoutSimulation {
    pub beams: Vec<Beam>,
    /// `(component index, power summed over all wavelengths)`.
    pub detector_power: Vec<(usize, f64)>,
    pub error: Option<String>,
}

/// How a component scatters light between the 8 directions, or `None` for
/// components that are not passive scatterers (sources, detectors).
fn scattering(kind: &OpticComponentKind) -> Option<Matrix> {
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
        | OpticComponentKind::Detector
        | OpticComponentKind::InputPort { .. }
        | OpticComponentKind::OutputPort => return None,
    }
    Some(s)
}

struct Element {
    node: NodeId,
    /// `(output port, direction)` pairs that can carry light.
    emits: Vec<(usize, usize)>,
    accepts_input: bool,
}

/// Simulate `components` (positions in panel space). `cell_um` is the
/// physical size of one grid cell; beams leaving the layout are drawn
/// `max_len` long.
/// A layout compiled into a circuit.
pub struct BuiltLayout {
    /// Circuit node of each component, by component index.
    pub nodes: Vec<NodeId>,
    pub segments: Vec<Segment>,
    /// Boundary ports from the layout's Input/Output port parts, in
    /// [`port_order`].
    pub ports: Ports,
}

/// A straight free-space path from one component towards the next.
pub struct Segment {
    pub start: Pos2,
    pub end: Pos2,
    pub dir: Vec2,
    /// Input of the free-space waveguide; its field is the field at `start`.
    pub input: InPort,
}

/// Indices of input and output port parts, ordered top to bottom, then left
/// to right — the order they appear on the component's box.
pub fn port_order(components: &[PlacedComponent]) -> (Vec<usize>, Vec<usize>) {
    let sorted = |want_input: bool| {
        let mut v: Vec<usize> = (0..components.len())
            .filter(|&i| match components[i].kind {
                OpticComponentKind::InputPort { .. } => want_input,
                OpticComponentKind::OutputPort => !want_input,
                _ => false,
            })
            .collect();
        v.sort_by(|&a, &b| {
            let (pa, pb) = (components[a].pos, components[b].pos);
            pa.y.total_cmp(&pb.y).then(pa.x.total_cmp(&pb.x))
        });
        v
    };
    (sorted(true), sorted(false))
}

/// Add the layout's components and free-space paths to `circuit`.
/// `grid_size` world units span `cell_um` micrometres; beams leaving the
/// layout are given length `max_len`.
pub fn build_layout(
    circuit: &mut Circuit,
    components: &[PlacedComponent],
    grid_size: f32,
    cell_um: f64,
    max_len: f32,
) -> BuiltLayout {
    let elements: Vec<Element> = components
        .iter()
        .map(|comp| match comp.kind {
            OpticComponentKind::Laser {
                amplitude,
                phase,
                wavelength,
                dir,
            } => Element {
                node: circuit
                    .add(Laser::new(amplitude as f64, wavelength as f64).with_phase(phase as f64)),
                emits: vec![(0, direction_index(dir))],
                accepts_input: false,
            },
            OpticComponentKind::Detector => Element {
                node: circuit.add(Detector::with_inputs(DIRECTIONS)),
                emits: Vec::new(),
                accepts_input: true,
            },
            // Light entering from outside the layout, launched along `dir`.
            OpticComponentKind::InputPort { dir } => Element {
                node: circuit.add(PhaseShifter::new(0.0)),
                emits: vec![(0, direction_index(dir))],
                accepts_input: false,
            },
            // Collects light arriving from any direction into one output.
            OpticComponentKind::OutputPort => Element {
                node: circuit.add(Custom::new(
                    "OutputPort",
                    Matrix::full(1, DIRECTIONS, C64::ONE),
                )),
                emits: Vec::new(),
                accepts_input: true,
            },
            ref kind => {
                let s = scattering(kind).expect("passive component");
                let emits = (0..DIRECTIONS)
                    .filter(|&k| (0..DIRECTIONS).any(|j| s[(k, j)] != C64::ZERO))
                    .map(|k| (k, k))
                    .collect();
                Element {
                    node: circuit.add(Custom::new(format!("{kind:?}"), s)),
                    emits,
                    accepts_input: true,
                }
            }
        })
        .collect();

    let mut segments = Vec::new();
    for (i, (comp, element)) in components.iter().zip(&elements).enumerate() {
        for &(port, k) in &element.emits {
            let d = direction(k);
            let (dx, dy) = (d.x as f64, d.y as f64);
            let hit = components
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .filter_map(|(j, other)| {
                    let (wx, wy) = (
                        (other.pos.x - comp.pos.x) as f64,
                        (other.pos.y - comp.pos.y) as f64,
                    );
                    let t = wx * dx + wy * dy;
                    let perp = (wx * dy - wy * dx).abs();
                    (t > 1.0 && perp < 0.5).then_some((j, t))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));

            let length_px = hit.map_or(max_len as f64, |(_, t)| t);
            let path = circuit.add(Waveguide::free_space(
                length_px / grid_size as f64 * cell_um,
            ));
            let from = OutPort {
                node: element.node,
                index: port,
            };
            circuit
                .connect(from, path.input(0))
                .expect("each output is used once");
            if let Some((j, _)) = hit
                && elements[j].accepts_input
            {
                circuit
                    .connect(path.output(0), elements[j].node.input(k))
                    .expect("nearest hit is unique per direction");
            }
            segments.push(Segment {
                start: comp.pos,
                end: hit.map_or(comp.pos + d * max_len, |(j, _)| components[j].pos),
                dir: d,
                input: path.input(0),
            });
        }
    }

    let (inputs, outputs) = port_order(components);
    BuiltLayout {
        ports: Ports {
            inputs: inputs.iter().map(|&i| elements[i].node.input(0)).collect(),
            outputs: outputs
                .iter()
                .map(|&i| elements[i].node.output(0))
                .collect(),
        },
        nodes: elements.into_iter().map(|e| e.node).collect(),
        segments,
    }
}

/// A free-space layout used as a block inside a larger circuit.
pub struct FreeSpaceBlock<'a> {
    pub components: &'a [PlacedComponent],
    pub cell_um: f64,
}

impl Block for FreeSpaceBlock<'_> {
    fn build(&self, circuit: &mut Circuit) -> Result<Ports, CircuitError> {
        Ok(build_layout(circuit, self.components, GRID, self.cell_um, GRID * 500.0).ports)
    }
}

/// Simulate `components` (positions in world space). `grid_size` world
/// units span `cell_um` micrometres; beams leaving the layout are drawn
/// `max_len` long.
pub fn simulate_layout(
    components: &[PlacedComponent],
    grid_size: f32,
    cell_um: f64,
    max_len: f32,
) -> LayoutSimulation {
    let mut circuit = Circuit::new();
    let built = build_layout(&mut circuit, components, grid_size, cell_um, max_len);

    let solutions = match circuit.simulate_sources() {
        Ok(s) => s,
        Err(e) => {
            return LayoutSimulation {
                error: Some(e.to_string()),
                ..Default::default()
            };
        }
    };

    let mut result = LayoutSimulation::default();
    for sol in &solutions {
        for seg in &built.segments {
            let field = sol.input_field(seg.input);
            if field.norm_sqr() > 1e-12 {
                result.beams.push(Beam {
                    start: seg.start,
                    end: seg.end,
                    dir: seg.dir,
                    wavelength_nm: sol.wavelength_nm(),
                    field,
                });
            }
        }
    }
    for (i, comp) in components.iter().enumerate() {
        let node = built.nodes[i];
        let power = match comp.kind {
            OpticComponentKind::Detector => solutions.iter().map(|s| s.detected_power(node)).sum(),
            OpticComponentKind::OutputPort => solutions
                .iter()
                .map(|s| s.output_field(node.output(0)).norm_sqr())
                .sum(),
            _ => continue,
        };
        result.detector_power.push((i, power));
    }
    result
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
            at(4.0, 2.0, OpticComponentKind::Detector),
            at(2.0, 4.0, OpticComponentKind::Detector),
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
        assert!(scattering(&OpticComponentKind::Detector).is_none());
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
                at(5.0, 0.0, OpticComponentKind::Detector),
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
            at(3.0, 0.0, OpticComponentKind::Detector),
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
            at(4.0, 0.0, OpticComponentKind::Detector),
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
            at(3.0, 3.0, OpticComponentKind::Detector),
            // Off the diagonal: must not be hit.
            at(3.0, 2.0, OpticComponentKind::Detector),
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
            at(2.0, 0.0, OpticComponentKind::Detector),
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
