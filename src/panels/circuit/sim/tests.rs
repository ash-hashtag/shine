//! Tests for the mixed-signal simulator.

use egui::Vec2;

use super::*;
use crate::panels::optics::examples;
use crate::panels::optics::model::{DETECTOR, Export, PlacedComponent, PortRef, cell_center};
use crate::panels::project::Project;

fn part(i: i32, j: i32, kind: OpticComponentKind) -> PlacedComponent {
    PlacedComponent { kind, pos: cell_center(i, j) }
}

fn laser(dx: f32, dy: f32) -> OpticComponentKind {
    OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(dx, dy), source: 0, color: None }
}

fn t(item: Item, port: usize) -> Terminal {
    Terminal { item, port }
}

fn run(layout: &Layout) -> Outcome {
    run_with(layout, &NoDocuments, &Machine::default(), None)
}

fn run_with(layout: &Layout, docs: &dyn Documents, machine: &Machine, root: Option<DocId>) -> Outcome {
    let o = simulate(layout, 10.0, docs, machine, root, &Options::default());
    assert!(o.error.is_none(), "{:?}", o.error);
    o
}

fn number(bits: u32, value: u64) -> BlockRef {
    BlockRef::Builtin(Builtin::NumberInput { bits, value })
}

fn display(bits: u32) -> BlockRef {
    BlockRef::Builtin(Builtin::Display { bits })
}

fn function(inputs: u32, light_inputs: u32, outputs: &[&str], light: &[&str]) -> BlockRef {
    BlockRef::Builtin(Builtin::FunctionCell {
        name: "f".into(),
        inputs,
        light_inputs,
        outputs: outputs.iter().map(|s| s.to_string()).collect(),
        light_outputs: light.iter().map(|s| s.to_string()).collect(),
        wavelength_nm: 1550.0,
    })
}

/// Number arriving at a display (or register) box's first `bits` inputs.
fn shown(o: &Outcome, layout: &Layout, item: Item, bits: usize) -> u64 {
    bits_to_number(&(0..bits).map(|k| o.input(layout, t(item, k))).collect::<Vec<_>>())
}

#[test]
fn number_inputs_drive_displays() {
    let mut l = Layout::default();
    let n = Item::Box(l.add_instance(number(4, 0b1011), (0, 0)));
    let d = Item::Box(l.add_instance(display(4), (0, 3)));
    for k in 0..4 {
        l.connect_signal(t(n, k), t(d, k));
    }
    let o = run(&l);
    assert_eq!(shown(&o, &l, d, 4), 11);
    assert_eq!(o.signal(n, 0), 1.0, "most significant bit first");
    assert_eq!(o.signal(n, 1), 0.0);
    assert!(o.converged && o.iterations >= 2);
    assert!(o.wavelengths.is_empty(), "no light at all");
}

#[test]
fn function_cells_compute_and_chain() {
    let mut l = Layout::default();
    let n = Item::Box(l.add_instance(number(8, 6), (0, 0)));
    let m = Item::Box(l.add_instance(number(8, 7), (10, 0)));
    let to_value = Item::Box(l.add_instance(function(8, 0, &["sum(in1*128, in2*64, in3*32, in4*16, in5*8, in6*4, in7*2, in8)"], &[]), (0, 3)));
    let to_value2 = Item::Box(l.add_instance(function(8, 0, &["sum(in1*128, in2*64, in3*32, in4*16, in5*8, in6*4, in7*2, in8)"], &[]), (10, 3)));
    let product = Item::Box(l.add_instance(function(2, 0, &["in1 * in2", "in1 > in2"], &[]), (5, 6)));
    for k in 0..8 {
        l.connect_signal(t(n, k), t(to_value, k));
        l.connect_signal(t(m, k), t(to_value2, k));
    }
    l.connect_signal(t(to_value, 0), t(product, 0));
    l.connect_signal(t(to_value2, 0), t(product, 1));
    let o = run(&l);
    assert_eq!(o.signal(product, 0), 42.0);
    assert_eq!(o.signal(product, 1), 0.0);
}

#[test]
fn function_cell_errors_are_reported() {
    let mut l = Layout::default();
    l.add_instance(function(1, 0, &["in1 +"], &[]), (0, 0));
    let o = simulate(&l, 10.0, &NoDocuments, &Machine::default(), None, &Options::default());
    let e = o.error.unwrap();
    assert!(e.starts_with("f, out1:") && e.contains("end of expression"), "{e}");
}

#[test]
fn signals_drive_lasers_and_phase_shifters() {
    // A free-space Mach–Zehnder whose phase shifter is driven by a bit.
    let mut l = examples::mach_zehnder();
    let shifter = l.components.iter().position(|p| matches!(p.kind, OpticComponentKind::PhaseShifter { .. })).unwrap();
    let bit = Item::Box(l.add_instance(number(1, 0), (0, -4)));
    l.connect_signal(t(bit, 0), t(Item::Part(shifter), 0));
    let detectors: Vec<usize> = (0..l.components.len()).filter(|&i| matches!(l.components[i].kind, OpticComponentKind::Detector { .. })).collect();
    let off = run(&l);
    let on = {
        let mut l = l.clone();
        l.instances[0].block = number(1, 1);
        run(&l)
    };
    let p = |o: &Outcome, i: usize| o.reading(detectors[i]).unwrap();
    assert!((p(&off, 0) - p(&on, 1)).abs() < 1e-9 && (p(&off, 1) - p(&on, 0)).abs() < 1e-9, "π flips the outputs");

    // A laser's amplitude follows its drive.
    let mut l = Layout::with_components(vec![part(0, 0, laser(1.0, 0.0)), part(3, 0, DETECTOR)]);
    let level = Item::Box(l.add_instance(function(0, 0, &["0.5"], &[]), (0, -3)));
    l.connect_signal(t(level, 0), t(Item::Part(0), 0));
    let o = run(&l);
    assert!((o.reading(1).unwrap() - 0.25).abs() < 1e-12, "amplitude 0.5 is power 0.25");
    assert!((o.signal(Item::Part(1), 0) - 0.25).abs() < 1e-12, "the detector reports power");
}

#[test]
fn thresholded_detectors_output_bits() {
    let mut l = Layout::with_components(vec![
        part(0, 0, laser(1.0, 0.0)),
        part(3, 0, OpticComponentKind::Detector { threshold: 0.5 }),
        part(0, 2, laser(1.0, 0.0)),
        part(3, 2, OpticComponentKind::Attenuator { transmission: 0.3 }),
        part(5, 2, OpticComponentKind::Detector { threshold: 0.5 }),
    ]);
    let o = run(&l);
    assert_eq!(o.signal(Item::Part(1), 0), 1.0);
    assert_eq!(o.signal(Item::Part(4), 0), 0.0);
    // Light → bit → light: the second laser is switched by the first detector.
    l.components[3].kind = OpticComponentKind::Attenuator { transmission: 1.0 };
    l.connect_signal(t(Item::Part(1), 0), t(Item::Part(2), 0));
    assert_eq!(run(&l).signal(Item::Part(4), 0), 1.0);
}

#[test]
fn beams_enter_and_leave_boxes() {
    // Laser → coupler box (2 in, 2 out) → detectors on both output rows.
    let mut l = Layout::with_components(vec![part(0, 0, laser(1.0, 0.0)), part(6, 0, DETECTOR), part(6, 1, DETECTOR)]);
    let dc = l.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 }), (2, 0));
    let o = run(&l);
    assert!((o.reading(1).unwrap() - 0.5).abs() < 1e-12 && (o.reading(2).unwrap() - 0.5).abs() < 1e-12);
    assert!((o.port_power[&(dc, 0)] - 0.5).abs() < 1e-12);
    // The beam into the box stops at its left edge.
    let into = o.beams.iter().find(|b| b.start == cell_center(0, 0)).unwrap();
    assert_eq!(into.end.x, 2.0 * GRID);

    // Light hitting a box anywhere but an input row is absorbed.
    let mut l = Layout::with_components(vec![part(3, -3, laser(0.0, 1.0)), part(3, 4, DETECTOR)]);
    l.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 }), (2, 0));
    assert_eq!(run(&l).reading(1), Some(0.0));
}

#[test]
fn waveguides_connect_boxes() {
    let mut l = Layout::with_components(vec![part(0, 0, laser(1.0, 0.0)), part(12, 0, DETECTOR)]);
    let a = l.add_instance(BlockRef::Builtin(Builtin::YSplitter), (2, 0));
    let b = l.add_instance(BlockRef::Builtin(Builtin::Attenuator { loss_db: 10.0 * 2f64.log10() }), (8, 4));
    l.connect_optical(PortRef { instance: a, port: 1 }, PortRef { instance: b, port: 0 });
    let o = run(&l);
    assert!((o.wire_power(0) - 0.5).abs() < 1e-12);
    assert!((o.reading(1).unwrap() - 0.5).abs() < 1e-12, "upper output reaches the detector as a beam");
    assert!((o.port_power[&(b, 0)] - 0.25).abs() < 1e-9, "lower output went through the waveguide and a 3 dB attenuator");

    // A wire into an input takes priority over a beam aimed at it.
    let mut wired = l.clone();
    wired.wires.clear();
    wired.connect_optical(PortRef { instance: a, port: 1 }, PortRef { instance: a, port: 0 });
    let e = simulate(&wired, 10.0, &NoDocuments, &Machine::default(), None, &Options::default());
    assert!(e.error.is_none() || e.error.as_deref().unwrap().contains("steady"), "{:?}", e.error);

    let mut broken = l.clone();
    broken.wires[0].from.port = 9;
    assert!(simulate(&broken, 10.0, &NoDocuments, &Machine::default(), None, &Options::default()).error.unwrap().contains("no longer exists"));
}

fn full_adder_project() -> (Project, DocId) {
    let mut p = Project::new();
    let fa = examples::import_component(&mut p, 0).unwrap();
    (p, fa)
}

#[test]
fn full_adder_truth_table() {
    let (p, fa) = full_adder_project();
    assert_eq!(p.doc(fa).unwrap().interface().signal_inputs, vec!["a", "b", "cin"]);
    assert_eq!(p.doc(fa).unwrap().interface().signal_outputs, vec!["sum", "cout"]);
    for bits in 0..8u64 {
        let mut l = Layout::default();
        let inputs: Vec<Item> = (0..3).map(|k| Item::Box(l.add_instance(number(1, (bits >> (2 - k)) & 1), (k * 3, 0)))).collect();
        let adder = Item::Box(l.add_instance(BlockRef::Component(fa), (0, 3)));
        let out = Item::Box(l.add_instance(display(2), (0, 6)));
        for (k, &input) in inputs.iter().enumerate() {
            l.connect_signal(t(input, 0), t(adder, k));
        }
        l.connect_signal(t(adder, 1), t(out, 0));
        l.connect_signal(t(adder, 0), t(out, 1));
        let o = run_with(&l, &p, &Machine::default(), None);
        assert_eq!(shown(&o, &l, out, 2), bits.count_ones() as u64, "inputs {bits:03b}");
    }
}

#[test]
fn eight_bit_adder_adds() {
    let mut p = Project::new();
    let id = examples::eight_bit_adder(&mut p);
    let layout = p.doc(id).unwrap().layout().clone();
    let display = layout.instances.iter().find(|i| matches!(i.block, BlockRef::Builtin(Builtin::Display { .. }))).unwrap().id;
    for (a, b) in [(100u64, 55u64), (0, 0), (255, 1), (255, 255), (170, 85), (1, 127), (200, 99)] {
        let mut l = layout.clone();
        l.instances[0].block = number(8, a);
        l.instances[1].block = number(8, b);
        let o = simulate(&l, 10.0, &p, &Machine::default(), Some(id), &Options::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        assert_eq!(shown(&o, &l, Item::Box(display), 9), a + b, "{a} + {b} after {} rounds", o.iterations);
    }
}

#[test]
fn accumulator_counts_on_the_clock() {
    let mut p = Project::new();
    let id = examples::accumulator(&mut p);
    let layout = p.doc(id).unwrap().layout().clone();
    let register = Item::Box(layout.instances[0].id);
    let total = |o: &Outcome| bits_to_number(&(0..8).map(|k| o.signal(register, k)).collect::<Vec<_>>());
    let mut machine = Machine::default();
    let mut totals = Vec::new();
    for _ in 0..5 {
        let o = simulate(&layout, 10.0, &p, &machine, Some(id), &Options::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        totals.push(total(&o));
        clock(&layout, &p, &mut machine, &o);
    }
    assert_eq!(totals, vec![0, 3, 6, 9, 12]);
    assert_eq!(machine.tick, 5);

    // Wraps around at 256.
    let register = layout.instances[0].id;
    machine.registers.insert((Vec::new(), register), (0..8).map(|k| if k < 7 { 1.0 } else { 0.0 }).collect());
    let o = simulate(&layout, 10.0, &p, &machine, Some(id), &Options::default());
    assert_eq!(total(&o), 254);
    clock(&layout, &p, &mut machine, &o);
    let o = simulate(&layout, 10.0, &p, &machine, Some(id), &Options::default());
    assert_eq!(total(&o), 1, "254 + 3 = 257 = 1 mod 256");
}

#[test]
fn registers_respect_write_enable() {
    let mut l = Layout::default();
    let value = Item::Box(l.add_instance(number(2, 3), (0, 0)));
    let write = Item::Box(l.add_instance(number(1, 0), (4, 0)));
    let reg = Item::Box(l.add_instance(BlockRef::Builtin(Builtin::Register { bits: 2 }), (0, 3)));
    l.connect_signal(t(value, 0), t(reg, 0));
    l.connect_signal(t(value, 1), t(reg, 1));
    l.connect_signal(t(write, 0), t(reg, 2));
    let mut m = Machine::default();
    let o = run_with(&l, &NoDocuments, &m, None);
    clock(&l, &NoDocuments, &mut m, &o);
    assert_eq!(run_with(&l, &NoDocuments, &m, None).signal(reg, 1), 0.0, "write low: unchanged");
    l.instances[1].block = number(1, 1);
    let o = run_with(&l, &NoDocuments, &m, None);
    clock(&l, &NoDocuments, &mut m, &o);
    let o = run_with(&l, &NoDocuments, &m, None);
    assert_eq!((o.signal(reg, 0), o.signal(reg, 1)), (1.0, 1.0));
}

#[test]
fn function_cells_emit_and_sense_light() {
    // A cell turns a number into light; another reads that light back.
    let mut l = Layout::default();
    let level = Item::Box(l.add_instance(number(2, 2), (0, 0)));
    let source = l.add_instance(function(2, 0, &[], &["sqrt(in1 * 2 + in2) / 2"]), (0, 3));
    let sensor = l.add_instance(function(0, 1, &["p1"], &[]), (6, 3));
    l.connect_signal(t(level, 0), t(Item::Box(source), 0));
    l.connect_signal(t(level, 1), t(Item::Box(source), 1));
    let o = run(&l);
    assert!((o.signal(Item::Box(sensor), 0) - 0.5).abs() < 1e-12, "√2/2 amplitude is 0.5 power");
    assert_eq!(o.wavelengths, vec![1550.0]);
}

#[test]
fn oscillating_loops_are_reported() {
    let mut l = Layout::default();
    let not = Item::Box(l.add_instance(function(1, 0, &["!in1"], &[]), (0, 0)));
    l.connect_signal(t(not, 0), t(not, 0));
    let o = simulate(&l, 10.0, &NoDocuments, &Machine::default(), None, &Options::default());
    assert!(o.error.unwrap().contains("did not settle"));
}

#[test]
fn nested_components_and_their_errors() {
    let mut p = Project::new();
    let fa = examples::import_component(&mut p, 0).unwrap();
    // A component containing a full adder, exposing its pins.
    let mut wrapper = Layout::with_components(vec![
        part(0, -2, OpticComponentKind::SignalIn),
        part(1, -2, OpticComponentKind::SignalIn),
        part(2, -2, OpticComponentKind::SignalIn),
        part(0, 4, OpticComponentKind::SignalOut),
    ]);
    let inner = Item::Box(wrapper.add_instance(BlockRef::Component(fa), (0, 0)));
    for k in 0..3 {
        wrapper.connect_signal(t(Item::Part(k), 0), t(inner, k));
    }
    wrapper.connect_signal(t(inner, 1), t(Item::Part(3), 0));
    let wrap = p.add_component("Carry only", "C", "", wrapper, 10.0);

    let mut top = Layout::default();
    let ones = Item::Box(top.add_instance(number(3, 0b110), (0, 0)));
    let w = Item::Box(top.add_instance(BlockRef::Component(wrap), (0, 3)));
    for k in 0..3 {
        top.connect_signal(t(ones, k), t(w, k));
    }
    let o = run_with(&top, &p, &Machine::default(), None);
    assert_eq!(o.signal(w, 0), 1.0, "carry of 1 + 1 + 0");
    assert!(o.signals.keys().any(|k| k.path.len() == 2), "signals inside nested components are recorded");

    // Self-containment and deleted components.
    let o = simulate(&top, 10.0, &p, &Machine::default(), Some(wrap), &Options::default());
    assert!(o.error.unwrap().contains("contains itself"));
    let mut missing = Layout::default();
    missing.add_instance(BlockRef::Component(DocId(999)), (0, 0));
    assert!(simulate(&missing, 10.0, &p, &Machine::default(), None, &Options::default()).error.unwrap().contains("deleted"));
}

#[test]
fn interface_orders_ports_and_pins() {
    let mut l = Layout::with_components(vec![
        part(0, 5, OpticComponentKind::SignalIn),
        part(3, 1, OpticComponentKind::SignalIn),
        part(0, 0, OpticComponentKind::InputPort { dir: Vec2::new(1.0, 0.0) }),
        part(9, 9, OpticComponentKind::OutputPort),
        part(1, 1, OpticComponentKind::SignalOut),
    ]);
    let pin = l.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "x".into() }), (0, 2));
    let i = interface(&l);
    assert_eq!(i.signal_in, vec![1, 0], "top to bottom");
    assert_eq!(i.optical_in, vec![Item::Part(2), Item::Box(pin)]);
    assert_eq!(i.counts(), PortCounts { optical_in: 2, optical_out: 1, signal_in: 2, signal_out: 1 });
    assert_eq!(block_counts(&BlockRef::Component(DocId(1)), &NoDocuments), PortCounts::default());
    assert_eq!(NoDocuments.name(DocId(4)), "component 4");
}

#[test]
fn helpers() {
    assert_eq!(bits_to_number(&[1.0, 0.0, 1.0, 1.0]), 11);
    assert_eq!(bits_to_number(&[]), 0);
    assert!(Outcome::failed("x").error.is_some());
    let o = Outcome::default();
    assert_eq!(o.wire_power(3), 0.0);
    assert_eq!(o.reading(0), None);
}

#[test]
fn wdm_link_carries_every_value_on_separate_wavelengths() {
    let mut p = Project::new();
    let id = examples::wdm_link(&mut p);
    let layout = p.doc(id).unwrap().layout().clone();
    let display = Item::Box(layout.instances.iter().find(|i| matches!(i.block, BlockRef::Builtin(Builtin::Display { .. }))).unwrap().id);
    let detectors: Vec<usize> =
        (0..layout.components.len()).filter(|&i| matches!(layout.components[i].kind, OpticComponentKind::Detector { .. })).collect();
    assert_eq!(detectors.len(), 4);
    for value in 0..16u64 {
        let mut l = layout.clone();
        l.instances[0].block = number(4, value);
        let o = run_with(&l, &p, &Machine::default(), Some(id));
        assert_eq!(shown(&o, &l, display, 4), value, "sent {value:04b}");
        if value.count_ones() == 1 {
            // One channel lit: it arrives almost whole, the others stay dark.
            let lit = value.leading_zeros() as usize - 60;
            for (k, &d) in detectors.iter().enumerate() {
                let power = o.reading(d).unwrap();
                if k == lit {
                    assert!(power > 0.9, "channel {k} received {power}");
                } else {
                    assert!(power < 0.01, "crosstalk {power} on channel {k}");
                }
            }
        }
    }
    // Detuning the receiver's ring for a channel loses that bit.
    let mut l = layout.clone();
    l.instances[0].block = number(4, 0b1111);
    let receiver_ring = l.instances.iter().position(|i| i.cell == (24, 4)).unwrap();
    if let BlockRef::Builtin(Builtin::AddDropRing { length_um, .. }) = &mut l.instances[receiver_ring].block {
        *length_um *= 1.002;
    }
    let o = run_with(&l, &p, &Machine::default(), Some(id));
    assert_eq!(shown(&o, &l, display, 4), 0b0111, "the first channel is no longer dropped");
}

#[test]
fn only_lasers_on_a_shared_source_interfere() {
    // The coherent XOR gate with both inputs on: shared source → the light
    // cancels on one detector; independent lasers → the powers just add.
    let shared = examples::xor_gate(true, true);
    let mut independent = shared.clone();
    for c in &mut independent.components {
        if let OpticComponentKind::Laser { source, .. } = &mut c.kind {
            *source = 0;
        }
    }
    let readings = |l: &Layout| {
        let o = run(l);
        let detectors: Vec<f64> = (0..l.components.len()).filter_map(|i| o.reading(i)).collect();
        (detectors, o.wavelengths.len())
    };
    let (coherent, channels) = readings(&shared);
    assert_eq!(channels, 1, "one source, one solve");
    assert!(coherent.iter().any(|&p| p < 1e-9), "interference cancels a port: {coherent:?}");
    let (incoherent, channels) = readings(&independent);
    assert_eq!(channels, 2, "two independent sources");
    assert!(incoherent.iter().all(|&p| (p - 1.0).abs() < 1e-9), "each port gets half of each: {incoherent:?}");
    assert!((coherent.iter().sum::<f64>() - incoherent.iter().sum::<f64>()).abs() < 1e-9, "same total power");
}

#[test]
fn laser_colours_reach_the_beams() {
    let mut l = Layout::with_components(vec![part(0, 0, laser(1.0, 0.0)), part(4, 0, DETECTOR)]);
    if let OpticComponentKind::Laser { color, .. } = &mut l.components[0].kind {
        *color = Some([10, 200, 30]);
    }
    let o = run(&l);
    assert_eq!(o.channel_colors, vec![Some([10, 200, 30])]);
    assert!(o.beams.iter().all(|b| b.color == Some([10, 200, 30])));
}

#[test]
fn exported_parameters_are_set_per_instance() {
    // The example Mach–Zehnder component exports its phase shifter's phase.
    // Two boxes of it, fed by lasers, get different phases: at 0 the light
    // leaves one output, at π the other.
    let mut p = Project::new();
    let mz = examples::import_component(&mut p, 1).unwrap();
    let inner = p.doc(mz).unwrap().layout().clone();
    let shifter = inner.components.iter().position(|c| matches!(c.kind, OpticComponentKind::PhaseShifter { .. })).unwrap();
    {
        let l = p.doc_mut(mz).unwrap().panel_mut();
        l.layout.exports = vec![Export { name: "phase".into(), item: Item::Part(shifter), param: "phase".into() }];
    }
    let mut top = Layout::default();
    let mut boxes = Vec::new();
    for (k, phase) in [None, Some(std::f64::consts::PI)].into_iter().enumerate() {
        let row = 6 * k as i32;
        top.components.push(part(0, row, laser(1.0, 0.0)));
        let id = top.add_instance(BlockRef::Component(mz), (3, row));
        top.connect_feed(k, PortRef { instance: id, port: 0 });
        let inst = top.instances.iter_mut().find(|i| i.id == id).unwrap();
        inst.set_param("phase", phase);
        boxes.push(id);
    }
    let o = run_with(&top, &p, &Machine::default(), None);
    assert!(o.error.is_none(), "{:?}", o.error);
    let out = |id, k| o.port_power.get(&(id, k)).copied().unwrap_or(0.0);
    let (a, b) = ((out(boxes[0], 0), out(boxes[0], 1)), (out(boxes[1], 0), out(boxes[1], 1)));
    assert!((a.0 + a.1 - 1.0).abs() < 1e-9 && (b.0 + b.1 - 1.0).abs() < 1e-9, "{a:?} {b:?}");
    assert!((a.0 - b.1).abs() < 1e-9 && (a.1 - b.0).abs() < 1e-9, "π swaps the outputs: {a:?} {b:?}");
    assert!(a.0.max(a.1) > 0.99, "a switch, not a splitter: {a:?}");

    // Values for names the component doesn't export are ignored; no values
    // means the component is used as it is.
    assert!(configure(&inner, &[]).is_none());
    let mut odd = top.clone();
    odd.instances[0].set_param("nonsense", Some(5.0));
    assert_eq!(run_with(&odd, &p, &Machine::default(), None).port_power, o.port_power);
    assert_eq!(top.instances[1].param("phase"), Some(std::f64::consts::PI));
    assert_eq!(top.instances[0].param("phase"), None);
}

#[test]
fn exports_follow_their_part_and_go_with_it() {
    let mut l = Layout::with_components(vec![part(0, 0, DETECTOR), part(2, 0, OpticComponentKind::PhaseShifter { phase: 0.0 })]);
    let ring = l.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 }), (5, 0));
    l.exports = vec![
        Export { name: "phase".into(), item: Item::Part(1), param: "phase".into() },
        Export { name: "split".into(), item: Item::Box(ring), param: "coupling".into() },
    ];
    l.remove_parts(&[0]);
    assert_eq!(l.exports[0].item, Item::Part(0), "indices shift with removals");
    l.remove_parts(&[0]);
    l.remove_instances(&[ring]);
    assert!(l.exports.is_empty(), "deleting what's exported removes the export");
}
