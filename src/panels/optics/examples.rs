//! Ready-made free-space layouts for the editor.

use egui::Vec2;

use super::model::{
    BlockRef, Builtin, Item, Layout, OpticComponentKind, PlacedComponent, PortRef, Terminal, cell_center,
};
use crate::panels::project::{DocId, Project};

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

const DETECTOR: OpticComponentKind = OpticComponentKind::Detector { threshold: 0.0 };

fn layout(components: Vec<PlacedComponent>) -> Layout {
    Layout::with_components(components)
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

// ---- components -----------------------------------------------------------------

/// The same layout with its laser replaced by an input port and its
/// detectors by output ports, so it can be used as a component.
fn as_component(mut layout: Layout) -> Layout {
    for part in &mut layout.components {
        part.kind = match part.kind {
            OpticComponentKind::Laser { dir, .. } => OpticComponentKind::InputPort { dir },
            OpticComponentKind::Detector { .. } => OpticComponentKind::OutputPort,
            other => other,
        };
    }
    layout
}

/// Index of the part at cell `(i, j)`.
fn part_at(layout: &Layout, i: i32, j: i32) -> usize {
    layout
        .components
        .iter()
        .position(|p| p.pos == cell_center(i, j))
        .unwrap_or_else(|| panic!("no part at ({i}, {j})"))
}

fn wire(layout: &mut Layout, from: (usize, usize), to: (usize, usize)) {
    layout.connect_signal(
        Terminal { item: Item::Part(from.0), port: from.1 },
        Terminal { item: Item::Part(to.0), port: to.1 },
    );
}

/// A one-bit full adder made of optics.
///
/// - Sum: a Mach–Zehnder with three phase shifters in one arm, each adding π
///   when its bit is 1. Light leaves the lower output when the total phase
///   is an odd multiple of π, so `sum = a XOR b XOR cin`.
/// - Carry: three lasers, switched by the bits, shine into one detector that
///   outputs 1 when it receives 2 or more units of power, so
///   `cout = majority(a, b, cin)`.
pub fn full_adder() -> Layout {
    let signal_in = OpticComponentKind::SignalIn;
    let signal_out = OpticComponentKind::SignalOut;
    let modulated = |dx: f32, dy: f32| OpticComponentKind::Laser {
        amplitude: 1.0,
        phase: 0.0,
        wavelength: 1550.0,
        dir: Vec2::new(dx, dy),
    };
    let mut l = layout(vec![
        at(0, -3, signal_in),
        at(1, -3, signal_in),
        at(2, -3, signal_in),
        // XOR: a balanced interferometer with the bits as phase shifts.
        at(0, 0, modulated(1.0, 0.0)),
        at(2, 0, splitter(-1.0, 1.0, 0.5)),
        at(3, 0, shifter(0.0)),
        at(4, 0, shifter(0.0)),
        at(5, 0, shifter(0.0)),
        at(6, 0, mirror(-1.0, 1.0)),
        at(2, 2, mirror(1.0, -1.0)),
        at(6, 2, splitter(-1.0, 1.0, 0.5)),
        at(6, 4, OpticComponentKind::Detector { threshold: 0.5 }),
        // Majority: three switched lasers into one thresholded detector.
        at(9, 5, modulated(1.0, 0.0)),
        at(11, 3, modulated(0.0, 1.0)),
        at(11, 7, modulated(0.0, -1.0)),
        at(11, 5, OpticComponentKind::Detector { threshold: 1.5 }),
        at(0, 9, signal_out),
        at(1, 9, signal_out),
    ]);
    let (a, b, c) = (part_at(&l, 0, -3), part_at(&l, 1, -3), part_at(&l, 2, -3));
    let (pa, pb, pc) = (part_at(&l, 3, 0), part_at(&l, 4, 0), part_at(&l, 5, 0));
    let (la, lb, lc) = (part_at(&l, 9, 5), part_at(&l, 11, 3), part_at(&l, 11, 7));
    let (sum_det, carry_det) = (part_at(&l, 6, 4), part_at(&l, 11, 5));
    let (sum, carry) = (part_at(&l, 0, 9), part_at(&l, 1, 9));
    for (bit, shift, source) in [(a, pa, la), (b, pb, lb), (c, pc, lc)] {
        wire(&mut l, (bit, 0), (shift, 0));
        wire(&mut l, (bit, 0), (source, 0));
    }
    wire(&mut l, (sum_det, 0), (sum, 0));
    wire(&mut l, (carry_det, 0), (carry, 0));
    for (part, label) in [(a, "a"), (b, "b"), (c, "cin"), (sum, "sum"), (carry, "cout")] {
        l.set_label(part, label);
    }
    l
}

pub struct ComponentExample {
    pub name: &'static str,
    pub initials: &'static str,
    pub description: &'static str,
    pub cell_um: f64,
    pub build: fn() -> Layout,
}

/// Components that ship with Shine, built from primitives. Placing one adds
/// its layout to the project, so it can be opened (Tab) and changed.
pub fn component_examples() -> Vec<ComponentExample> {
    vec![
        ComponentExample {
            name: "Full adder",
            initials: "FA",
            description: "Adds three bits with light: an interferometer computes the sum (XOR) and a thresholded detector the carry (majority). Chain eight for an 8-bit adder.",
            cell_um: 10.0,
            build: full_adder,
        },
        ComponentExample {
            name: "Mach–Zehnder interferometer",
            initials: "MZI",
            description: "One input, two outputs. The phase shifter inside chooses which output gets the light: a switch or modulator.",
            cell_um: 10.0,
            build: || as_component(mach_zehnder()),
        },
        ComponentExample {
            name: "Fabry–Pérot filter",
            initials: "FP",
            description: "A resonant cavity: passes light only near its resonant wavelengths.",
            cell_um: FABRY_PEROT_CELL_UM,
            build: || as_component(fabry_perot()),
        },
        ComponentExample {
            name: "1→4 splitter",
            initials: "1:4",
            description: "Divides one beam equally over four outputs.",
            cell_um: 10.0,
            build: || as_component(fan_out()),
        },
    ]
}

/// The project's copy of an example component, adding it on first use.
pub fn import_component(project: &mut Project, example: usize) -> Option<DocId> {
    let ex = component_examples().into_iter().nth(example)?;
    if let Some(id) = project.find(ex.name).filter(|&id| project.doc(id).is_some_and(|d| d.component.is_some())) {
        return Some(id);
    }
    Some(project.add_component(ex.name, ex.initials, ex.description, (ex.build)(), ex.cell_um))
}

// ---- demos -------------------------------------------------------------------------

fn signal(layout: &mut Layout, from: (Item, usize), to: (Item, usize)) {
    layout.connect_signal(Terminal { item: from.0, port: from.1 }, Terminal { item: to.0, port: to.1 });
}

/// Eight full adders in a row (most significant on the left), carries
/// rippling from right to left. Returns the adder boxes, bit 0 first.
fn adder_row(layout: &mut Layout, full_adder: DocId, row: i32) -> Vec<Item> {
    let adders: Vec<Item> = (0..8)
        .map(|k| Item::Box(layout.add_instance(BlockRef::Component(full_adder), ((7 - k) * 4, row))))
        .collect();
    for k in 0..7 {
        signal(layout, (adders[k as usize], 1), (adders[k as usize + 1], 2));
    }
    adders
}

/// Two 8-bit numbers added by eight optical full adders.
pub fn eight_bit_adder(project: &mut Project) -> DocId {
    let fa = import_component(project, 0).expect("full adder exists");
    let mut l = Layout::default();
    let a = Item::Box(l.add_instance(BlockRef::Builtin(Builtin::NumberInput { bits: 8, value: 100 }), (0, 0)));
    let b = Item::Box(l.add_instance(BlockRef::Builtin(Builtin::NumberInput { bits: 8, value: 55 }), (12, 0)));
    let adders = adder_row(&mut l, fa, 4);
    let display = Item::Box(l.add_instance(BlockRef::Builtin(Builtin::Display { bits: 9 }), (9, 8)));
    for (k, &adder) in adders.iter().enumerate() {
        // Number ports are most significant first: bit k is port 7 - k.
        signal(&mut l, (a, 7 - k), (adder, 0));
        signal(&mut l, (b, 7 - k), (adder, 1));
        signal(&mut l, (adder, 0), (display, 8 - k));
    }
    signal(&mut l, (adders[7], 1), (display, 0));
    project.add_layout("8-bit photonic adder", l, 10.0)
}

/// A register feeding an adder that feeds the register back: every clock
/// tick adds the step to the running total.
pub fn accumulator(project: &mut Project) -> DocId {
    let fa = import_component(project, 0).expect("full adder exists");
    let mut l = Layout::default();
    let register = Item::Box(l.add_instance(BlockRef::Builtin(Builtin::Register { bits: 8 }), (0, 0)));
    let step = Item::Box(l.add_instance(BlockRef::Builtin(Builtin::NumberInput { bits: 8, value: 3 }), (12, 0)));
    let adders = adder_row(&mut l, fa, 4);
    // The register shows the running total itself.
    for (k, &adder) in adders.iter().enumerate() {
        signal(&mut l, (register, 7 - k), (adder, 0));
        signal(&mut l, (step, 7 - k), (adder, 1));
        signal(&mut l, (adder, 0), (register, 7 - k));
    }
    project.add_layout("Photonic accumulator", l, 10.0)
}

/// A free-space Mach–Zehnder used as a component twice, in cascade.
pub fn cascaded_interferometers(project: &mut Project) -> DocId {
    let mzi = import_component(project, 1).expect("MZI exists");
    let mut l = layout(vec![
        at(0, 0, laser(1.0, 0.0, 0.0)),
        at(14, 1, DETECTOR),
        at(14, 6, DETECTOR),
        at(14, 7, DETECTOR),
    ]);
    // A beam enters the first box; a waveguide carries its upper output to the
    // second box, and its lower output leaves as a beam.
    let first = l.add_instance(BlockRef::Component(mzi), (2, 0));
    let second = l.add_instance(BlockRef::Component(mzi), (8, 6));
    l.connect_optical(PortRef { instance: first, port: 0 }, PortRef { instance: second, port: 0 });
    project.add_layout("Cascaded interferometers", l, 10.0)
}

/// Channel wavelengths of the WDM link, 10 nm apart in the telecom C band.
pub const WDM_CHANNELS_NM: [f64; 4] = [1530.0, 1540.0, 1550.0, 1560.0];

/// An add–drop ring resonant at `nm` (20th order, n_eff 2.4).
fn channel_ring(nm: f64) -> Builtin {
    Builtin::AddDropRing {
        coupling: 0.05,
        length_um: 20.0 * nm / 1000.0 / 2.4,
        n_eff: 2.4,
        loss_db_per_cm: 2.0,
    }
}

/// Four bits sent over one waveguide on four wavelengths, the way silicon
/// photonic transceivers move data between servers.
///
/// Transmitter: each bit switches a laser at its own wavelength, and a ring
/// resonator tuned to that wavelength adds it to the shared bus. Receiver:
/// rings tuned to the same wavelengths drop each colour onto its own
/// threshold detector, which gives the bit back.
pub fn wdm_link(project: &mut Project) -> DocId {
    let mut l = Layout::default();
    let data = Item::Box(l.add_instance(BlockRef::Builtin(Builtin::NumberInput { bits: 4, value: 0b1011 }), (3, 0)));
    let mut bus: Option<PortRef> = None;
    for (k, &nm) in WDM_CHANNELS_NM.iter().enumerate() {
        let x = 4 + 4 * k as i32;
        let ring = l.add_instance(BlockRef::Builtin(channel_ring(nm)), (x, 4));
        // The laser shines into the ring's add port (its lower input).
        l.components.push(at(x - 1, 5, OpticComponentKind::Laser {
            amplitude: 1.0,
            phase: 0.0,
            wavelength: nm as f32,
            dir: Vec2::new(1.0, 0.0),
        }));
        let laser = l.components.len() - 1;
        signal(&mut l, (data, k), (Item::Part(laser), 0));
        if let Some(through) = bus {
            l.connect_optical(through, PortRef { instance: ring, port: 0 });
        }
        bus = Some(PortRef { instance: ring, port: 0 });
    }
    let received = Item::Box(l.add_instance(BlockRef::Builtin(Builtin::Display { bits: 4 }), (26, 9)));
    for (k, &nm) in WDM_CHANNELS_NM.iter().enumerate() {
        let x = 24 + 5 * k as i32;
        let ring = l.add_instance(BlockRef::Builtin(channel_ring(nm)), (x, 4));
        l.connect_optical(bus.expect("transmitter built"), PortRef { instance: ring, port: 0 });
        bus = Some(PortRef { instance: ring, port: 0 });
        // The drop port's light leaves the box's right edge into a detector.
        l.components.push(at(x + 2, 5, OpticComponentKind::Detector { threshold: 0.5 }));
        let detector = l.components.len() - 1;
        signal(&mut l, (Item::Part(detector), 0), (received, k));
    }
    project.add_layout("4-channel WDM optical link", l, 10.0)
}

pub struct Demo {
    pub name: &'static str,
    pub description: &'static str,
    pub build: fn(&mut Project) -> DocId,
}

/// Examples that combine components, electronics and optics.
pub fn demos() -> Vec<Demo> {
    vec![
        Demo {
            name: "4-channel WDM optical link",
            description: "What photonic chips do in data centres today: four bits travel down one waveguide on four colours of light. Each bit switches a laser at its own wavelength (1530–1560 nm); ring resonators tuned to those wavelengths add them to the shared waveguide, and identical rings at the receiver pick each colour off onto a detector that turns it back into a bit. Real transceivers run each wavelength at 25–100 Gb/s. Click the input bits, or select a ring and detune it to see the channel drop out.",
            build: wdm_link,
        },
        Demo {
            name: "8-bit photonic adder",
            description: "Adds two 8-bit numbers in an electro-optic teaching model. Each full adder computes its sum bit by interference (the input bits drive phase shifters) and its carry with a thresholded detector; carries ripple from right to left through electronics. Research chips keep the carry in light and detect only at the end, and in practice photonics is used for data links and linear algebra rather than digital arithmetic. Click the input bits; open a full adder with Tab to see its optics.",
            build: eight_bit_adder,
        },
        Demo {
            name: "Photonic accumulator",
            description: "A register holds a running total; the photonic adder adds the step to it and the register stores the result on each clock tick. Press Step (or Run) to count: the core of an arithmetic unit.",
            build: accumulator,
        },
        Demo {
            name: "Cascaded interferometers",
            description: "The free-space Mach–Zehnder saved as a component and used twice: a beam feeds the first box directly and a waveguide carries one output to the second.",
            build: cascaded_interferometers,
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
