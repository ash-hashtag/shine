//! Ready-made free-space layouts for the editor.

use egui::Vec2;

use super::model::{Layout, OpticComponentKind, PlacedComponent, cell_center};

pub struct LayoutExample {
    pub name: &'static str,
    pub description: &'static str,
    /// Physical cell size the example is tuned for.
    pub cell_um: f64,
    pub build: fn() -> Layout,
}

fn at(i: i32, j: i32, kind: OpticComponentKind) -> PlacedComponent {
    PlacedComponent {
        kind,
        pos: cell_center(i, j),
    }
}

fn laser(dx: f32, dy: f32, phase: f32) -> OpticComponentKind {
    OpticComponentKind::Laser {
        amplitude: 1.0,
        phase,
        wavelength: 650.0,
        dir: Vec2::new(dx, dy),
    }
}

fn splitter(nx: f32, ny: f32, reflectivity: f32) -> OpticComponentKind {
    OpticComponentKind::BeamSplitter {
        reflectivity,
        transparency: 1.0 - reflectivity,
        normal: Vec2::new(nx, ny).normalized(),
    }
}

fn mirror(nx: f32, ny: f32) -> OpticComponentKind {
    OpticComponentKind::Mirror {
        normal: Vec2::new(nx, ny).normalized(),
    }
}

fn shifter(phase: f32) -> OpticComponentKind {
    OpticComponentKind::PhaseShifter { phase }
}

const DETECTOR: OpticComponentKind = OpticComponentKind::Detector;

fn layout(components: Vec<PlacedComponent>) -> Layout {
    Layout {
        components,
        lines: Vec::new(),
    }
}

pub fn mach_zehnder() -> Layout {
    layout(vec![
        at(-2, 0, laser(1.0, 0.0, 0.0)),
        at(0, 0, splitter(-1.0, 1.0, 0.5)),
        at(1, 0, shifter(0.0)),
        at(2, 0, mirror(-1.0, 1.0)),
        at(0, 2, mirror(1.0, -1.0)),
        at(2, 2, splitter(-1.0, 1.0, 0.5)),
        at(4, 2, DETECTOR),
        at(2, 4, DETECTOR),
    ])
}

pub fn michelson() -> Layout {
    layout(vec![
        at(-3, 0, laser(1.0, 0.0, 0.0)),
        at(0, 0, splitter(-1.0, -1.0, 0.5)),
        at(1, 0, shifter(0.0)),
        at(3, 0, mirror(-1.0, 0.0)),
        at(0, -3, mirror(0.0, 1.0)),
        at(0, 3, DETECTOR),
    ])
}

/// Cell size putting the 2-cell Fabry-Perot gap on resonance at 650 nm.
pub const FABRY_PEROT_CELL_UM: f64 = 650.0 * 30.5 / 4000.0;

pub fn fabry_perot() -> Layout {
    layout(vec![
        at(-2, 0, laser(1.0, 0.0, 0.0)),
        at(0, 0, splitter(-1.0, 0.0, 0.9)),
        at(2, 0, splitter(-1.0, 0.0, 0.9)),
        at(4, 0, DETECTOR),
    ])
}

/// Bits are laser phases: 0 -> 0, pi -> 1.
pub fn xor_gate(a: bool, b: bool) -> Layout {
    let bit = |v: bool| if v { std::f32::consts::PI } else { 0.0 };
    layout(vec![
        at(-3, 0, laser(1.0, 0.0, bit(a))),
        at(-1, 0, shifter(std::f32::consts::FRAC_PI_2)),
        at(0, 0, splitter(-1.0, -1.0, 0.5)),
        at(0, 3, laser(0.0, -1.0, bit(b))),
        at(0, -3, DETECTOR),
        at(3, 0, DETECTOR),
    ])
}

pub fn fan_out() -> Layout {
    layout(vec![
        at(-2, 0, laser(1.0, 0.0, 0.0)),
        at(0, 0, splitter(-1.0, -1.0, 0.5)),
        at(3, 0, splitter(-1.0, -1.0, 0.5)),
        at(0, -3, splitter(-1.0, -1.0, 0.5)),
        at(5, 0, DETECTOR),
        at(3, -2, DETECTOR),
        at(2, -3, DETECTOR),
        at(0, -5, DETECTOR),
    ])
}

pub fn layout_examples() -> Vec<LayoutExample> {
    vec![
        LayoutExample {
            name: "Mach–Zehnder interferometer",
            description: "Splits a beam into two arms and recombines it. The phase shifter in the upper arm steers all of the light between the two detectors — the basic optical switch and modulator of photonic chips.",
            cell_um: 10.0,
            build: mach_zehnder,
        },
        LayoutExample {
            name: "Michelson interferometer",
            description: "Both arms are folded back by mirrors. Light passes the phase shifter twice, so a quarter-wave (π/2) setting already turns the detector dark. Used for precision length measurement.",
            cell_um: 10.0,
            build: michelson,
        },
        LayoutExample {
            name: "Fabry–Pérot cavity",
            description: "Two 90% mirrors form a resonator. On resonance all light is transmitted; change the laser wavelength or the cell size slightly to fall off the sharp resonance — the principle behind ring filters and lasers.",
            cell_um: FABRY_PEROT_CELL_UM,
            build: fabry_perot,
        },
        LayoutExample {
            name: "Coherent XOR gate",
            description: "Two lasers encode bits in their phase (0 → 0, π → 1). After a π/2 bias and a beam splitter, the top detector reads a XOR b (power 2 = 1) and the right detector reads XNOR. Set each laser's phase to 0 or π to try all inputs.",
            cell_um: 10.0,
            build: || xor_gate(false, true),
        },
        LayoutExample {
            name: "1→4 fan-out tree",
            description: "Three 50:50 splitters distribute one laser equally over four detectors, like the fan-out that feeds many weights in an optical neural network.",
            cell_um: 10.0,
            build: fan_out,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::beam_sim::{LayoutSimulation, simulate_layout};
    use crate::panels::optics::model::GRID;

    fn simulate(layout: &Layout, cell_um: f64) -> LayoutSimulation {
        let sim = simulate_layout(&layout.components, GRID, cell_um, 2000.0);
        assert!(sim.error.is_none(), "{:?}", sim.error);
        sim
    }

    /// Detector powers in layout order.
    fn readings(layout: &Layout, cell_um: f64) -> Vec<f64> {
        let sim = simulate(layout, cell_um);
        let mut readings: Vec<_> = sim.detector_power.clone();
        readings.sort_by_key(|r| r.0);
        readings.into_iter().map(|r| r.1).collect()
    }

    fn set_phase(layout: &mut Layout, phase: f32) {
        for c in &mut layout.components {
            if let OpticComponentKind::PhaseShifter { phase: p } = &mut c.kind {
                *p = phase;
            }
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn catalogue_is_well_formed() {
        let examples = layout_examples();
        assert_eq!(examples.len(), 5);
        for ex in examples {
            let layout = (ex.build)();
            assert!(!ex.name.is_empty() && ex.description.len() > 40);
            assert!(ex.cell_um > 0.0);
            // No two components share a cell.
            let mut cells: Vec<_> = layout
                .components
                .iter()
                .map(|c| super::super::model::cell_of(c.pos))
                .collect();
            cells.sort();
            cells.dedup();
            assert_eq!(cells.len(), layout.components.len(), "{}", ex.name);
            simulate(&layout, ex.cell_um);
        }
    }

    #[test]
    fn mach_zehnder_switches() {
        let mut l = mach_zehnder();
        let r = readings(&l, 10.0);
        assert!(close(r[0] + r[1], 1.0));
        assert!(r[0].max(r[1]) > 0.999999);
        set_phase(&mut l, std::f32::consts::PI);
        let s = readings(&l, 10.0);
        assert!(close(s[0], r[1]) && close(s[1], r[0]));
    }

    #[test]
    fn michelson_darkens_at_quarter_wave() {
        let mut l = michelson();
        assert!(close(readings(&l, 10.0)[0], 1.0));
        set_phase(&mut l, std::f32::consts::FRAC_PI_2);
        assert!(readings(&l, 10.0)[0] < 1e-6);
    }

    #[test]
    fn fabry_perot_matches_airy_function() {
        let l = fabry_perot();
        let r = 0.9;
        for (wavelength, cell) in [
            (650.0, FABRY_PEROT_CELL_UM),
            (650.0, FABRY_PEROT_CELL_UM * 1.002),
            (651.0, FABRY_PEROT_CELL_UM),
        ] {
            let mut l = l.clone();
            if let OpticComponentKind::Laser { wavelength: w, .. } = &mut l.components[0].kind {
                *w = wavelength;
            }
            let phi = 2.0 * std::f64::consts::PI * 2.0 * cell * 1000.0 / wavelength as f64;
            let expected = (1.0 - r) * (1.0 - r) / (1.0 + r * (2.0 * phi).cos() * 2.0 + r * r);
            let got = readings(&l, cell)[0];
            assert!(
                (got - expected).abs() < 1e-4,
                "λ={wavelength} cell={cell}: {got} vs {expected}"
            );
        }
        assert!(readings(&l, FABRY_PEROT_CELL_UM)[0] > 0.999);
    }

    #[test]
    fn xor_truth_table() {
        for (a, b) in [(false, false), (false, true), (true, false), (true, true)] {
            let r = readings(&xor_gate(a, b), 10.0);
            let (xor, xnor) = (r[0], r[1]);
            let expected = if a != b { (2.0, 0.0) } else { (0.0, 2.0) };
            assert!(
                close(xor, expected.0) && close(xnor, expected.1),
                "{a} {b}: {r:?}"
            );
        }
    }

    #[test]
    fn fan_out_is_equal() {
        for p in readings(&fan_out(), 10.0) {
            assert!(close(p, 0.25));
        }
    }
}
