//! Mixed-signal simulation of a layout: free-space optics, component boxes,
//! waveguides, and the electronics that drive and read them.
//!
//! Light is solved exactly as a linear circuit (see `photonics`). Detectors
//! turn light into signals, signals drive lasers, phase shifters and
//! attenuators, and function cells and registers compute in between. Each
//! [`simulate`] call settles that loop: electronics are evaluated from the
//! latest detector readings, the optics are rebuilt with the resulting drive
//! levels and solved again, until neither changes. This mirrors real
//! optoelectronic chips, where the optics are much faster than the
//! electronics around them. [`clock`] advances registers by one tick.
//!
//! Components nest: a box whose block is another document is simulated
//! inside the same circuit, with its signal pins and optical ports mapped to
//! the box's terminals.

use std::collections::HashMap;

use emath::{Pos2, Vec2};

use super::expr;
use crate::panels::beam_sim::{Beam, DIRECTIONS, direction, direction_index, scattering};
use crate::panels::optics::model::{
    BlockRef, Builtin, GRID, InstanceId, Item, Layout, OpticComponentKind, PortCounts, Terminal, box_geometry,
};
use crate::panels::project::DocId;
use crate::photonics::{C64, Circuit, Custom, Detector, InPort, Laser, Matrix, NodeId, OutPort, Ports, Waveguide};

/// Documents that boxes in a layout can refer to.
pub trait Documents {
    /// A layout used as a component, and its cell size in µm.
    fn layout(&self, id: DocId) -> Option<(&Layout, f64)>;
    fn name(&self, id: DocId) -> String;
}

/// For layouts that use only built-in blocks.
pub struct NoDocuments;

impl Documents for NoDocuments {
    fn layout(&self, _: DocId) -> Option<(&Layout, f64)> {
        None
    }
    fn name(&self, id: DocId) -> String {
        format!("component {}", id.0)
    }
}

/// Copies of the documents a layout uses (and those they use), so it can
/// be simulated on another thread while the editor carries on.
pub struct Snapshot {
    docs: HashMap<DocId, (Layout, f64, String)>,
}

impl Snapshot {
    pub fn of(layout: &Layout, docs: &dyn Documents) -> Self {
        let mut s = Self { docs: HashMap::new() };
        let mut todo: Vec<DocId> = Vec::new();
        let used = |l: &Layout, todo: &mut Vec<DocId>| {
            todo.extend(l.instances.iter().filter_map(|i| match i.block {
                BlockRef::Component(d) => Some(d),
                BlockRef::Builtin(_) => None,
            }))
        };
        used(layout, &mut todo);
        while let Some(id) = todo.pop() {
            if s.docs.contains_key(&id) {
                continue;
            }
            if let Some((l, cell)) = docs.layout(id) {
                used(l, &mut todo);
                s.docs.insert(id, (l.clone(), cell, docs.name(id)));
            }
        }
        s
    }
}

impl Documents for Snapshot {
    fn layout(&self, id: DocId) -> Option<(&Layout, f64)> {
        self.docs.get(&id).map(|(l, c, _)| (l, *c))
    }
    fn name(&self, id: DocId) -> String {
        self.docs.get(&id).map_or_else(|| "a deleted component".into(), |d| d.2.clone())
    }
}

/// Instances from the top layout down to a nested layout.
pub type Path = Vec<InstanceId>;

/// A terminal anywhere in the hierarchy.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub path: Path,
    pub item: Item,
    pub port: usize,
}

impl Key {
    pub fn new(path: &[InstanceId], item: Item, port: usize) -> Self {
        Self {
            path: path.to_vec(),
            item,
            port,
        }
    }
}

/// State that persists between simulations: register contents, the clock,
/// and values on the top layout's own signal-in pins (set by hand to try a
/// component on its own).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    pub registers: HashMap<(Path, InstanceId), Vec<f64>>,
    /// Scripted components' state, as JSON, by box (unset: the script's own
    /// initial state).
    pub scripts: HashMap<(Path, InstanceId), String>,
    pub tick: u64,
    /// By signal-in pin, in interface order; missing pins read 0.
    pub inputs: Vec<f64>,
}

/// The boundary of a layout used as a component, in the order its box shows them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interface {
    /// Input port parts or input-pin boxes, top to bottom.
    pub optical_in: Vec<Item>,
    pub optical_out: Vec<Item>,
    /// Signal-in and signal-out parts (indices), top to bottom.
    pub signal_in: Vec<usize>,
    pub signal_out: Vec<usize>,
}

impl Interface {
    pub fn counts(&self) -> PortCounts {
        PortCounts {
            optical_in: self.optical_in.len(),
            optical_out: self.optical_out.len(),
            signal_in: self.signal_in.len(),
            signal_out: self.signal_out.len(),
        }
    }
}

pub fn interface(layout: &Layout) -> Interface {
    let position = |item: &Item| match item {
        Item::Part(i) => layout.components[*i].pos,
        Item::Box(id) => layout
            .instance(*id)
            .map(|inst| Pos2::new(inst.cell.0 as f32 * GRID, inst.cell.1 as f32 * GRID))
            .unwrap_or_default(),
    };
    let sort = |mut items: Vec<Item>| {
        items.sort_by(|a, b| {
            let (pa, pb) = (position(a), position(b));
            pa.y.total_cmp(&pb.y).then(pa.x.total_cmp(&pb.x))
        });
        items
    };
    let parts = |want: fn(&OpticComponentKind) -> bool| -> Vec<Item> {
        (0..layout.components.len())
            .filter(|&i| want(&layout.components[i].kind))
            .map(Item::Part)
            .collect()
    };
    let pins = |want: fn(&Builtin) -> bool| -> Vec<Item> {
        layout
            .instances
            .iter()
            .filter(|inst| matches!(&inst.block, BlockRef::Builtin(b) if want(b)))
            .map(|inst| Item::Box(inst.id))
            .collect()
    };
    let mut optical_in = parts(|k| matches!(k, OpticComponentKind::InputPort { .. }));
    optical_in.extend(pins(|b| matches!(b, Builtin::InputPin { .. })));
    let mut optical_out = parts(|k| matches!(k, OpticComponentKind::OutputPort));
    optical_out.extend(pins(|b| matches!(b, Builtin::OutputPin { .. })));
    let indices = |items: Vec<Item>| {
        items
            .into_iter()
            .map(|i| match i {
                Item::Part(i) => i,
                Item::Box(_) => unreachable!("signal pins are parts"),
            })
            .collect()
    };
    Interface {
        optical_in: sort(optical_in),
        optical_out: sort(optical_out),
        signal_in: indices(sort(parts(|k| matches!(k, OpticComponentKind::SignalIn)))),
        signal_out: indices(sort(parts(|k| matches!(k, OpticComponentKind::SignalOut)))),
    }
}

/// Port counts of a block, looking documents up through `docs`.
pub fn block_counts(block: &BlockRef, docs: &dyn Documents) -> PortCounts {
    match block {
        BlockRef::Builtin(b) => PortCounts {
            optical_in: b.inputs().len(),
            optical_out: b.outputs().len(),
            signal_in: b.signal_inputs().len(),
            signal_out: b.signal_outputs().len(),
        },
        BlockRef::Component(id) => docs.layout(*id).map(|(l, _)| interface(l).counts()).unwrap_or_default(),
    }
}

/// Field on a waveguide at each wavelength.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// One light channel per entry: its wavelength. Channels are mutually
    /// incoherent (different wavelengths, or independent sources).
    pub wavelengths: Vec<f64>,
    /// Display colour chosen for each channel's source, if any.
    pub channel_colors: Vec<Option<[u8; 3]>>,
    /// Beams in the top layout.
    pub beams: Vec<Beam>,
    /// Power at each detector and output port of the top layout, by part index.
    pub detector_power: Vec<(usize, f64)>,
    /// Field at the start of each top-level waveguide, per wavelength.
    pub wire_fields: Vec<Vec<C64>>,
    /// Power leaving each optical output of top-level boxes.
    pub port_power: HashMap<(InstanceId, usize), f64>,
    /// Every signal output in the hierarchy.
    pub signals: HashMap<Key, f64>,
    /// Optics–electronics rounds needed to settle.
    pub iterations: usize,
    pub converged: bool,
    pub error: Option<String>,
}

impl Outcome {
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            error: Some(message.into()),
            ..Self::default()
        }
    }

    /// Value of a top-level signal output.
    pub fn signal(&self, item: Item, port: usize) -> f64 {
        self.signals.get(&Key::new(&[], item, port)).copied().unwrap_or(0.0)
    }

    /// Value arriving at a top-level signal input (0 when unconnected).
    pub fn input(&self, layout: &Layout, to: Terminal) -> f64 {
        layout.signal_source(to).map_or(0.0, |from| self.signal(from.item, from.port))
    }

    pub fn wire_power(&self, wire: usize) -> f64 {
        self.wire_fields.get(wire).map_or(0.0, |f| f.iter().map(|v| v.norm_sqr()).fold(0.0, |a, b| a + b))
    }

    pub fn reading(&self, part: usize) -> Option<f64> {
        self.detector_power.iter().find(|d| d.0 == part).map(|d| d.1)
    }
}

pub struct Options {
    /// Length drawn for beams that leave the layout.
    pub max_len: f32,
    /// Give up settling after this many optics–electronics rounds.
    pub max_iterations: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_len: GRID * 500.0,
            max_iterations: 64,
        }
    }
}

// ---- electronics --------------------------------------------------------------

struct Electronics<'a> {
    docs: &'a dyn Documents,
    machine: &'a Machine,
    powers: &'a HashMap<Key, f64>,
    signals: HashMap<Key, f64>,
    /// Light amplitude computed by function cells, keyed by their optical output.
    light: HashMap<Key, f64>,
    stack: Vec<DocId>,
    errors: Vec<String>,
}

impl Electronics<'_> {
    fn value(&self, path: &[InstanceId], layout: &Layout, to: Terminal) -> f64 {
        layout
            .signal_source(to)
            .and_then(|from| self.signals.get(&Key::new(path, from.item, from.port)).copied())
            .unwrap_or(0.0)
    }

    fn set(&mut self, path: &[InstanceId], item: Item, port: usize, v: f64) -> bool {
        let key = Key::new(path, item, port);
        let old = self.signals.insert(key, v);
        old.is_none_or(|o| (o - v).abs() > 1e-12 && !(o.is_nan() && v.is_nan()))
    }

    fn error(&mut self, message: String) {
        if !self.errors.contains(&message) {
            self.errors.push(message);
        }
    }

    /// Evaluate a layout's signals given its signal-in values; returns its
    /// signal-out values in interface order.
    fn eval(&mut self, layout: &Layout, path: &[InstanceId], inputs: &[f64]) -> Vec<f64> {
        let iface = interface(layout);
        let mut settled = false;
        for _pass in 0..64 {
            let mut changed = false;
            for (i, part) in layout.components.iter().enumerate() {
                match part.kind {
                    OpticComponentKind::Detector { threshold } => {
                        let p = self.powers.get(&Key::new(path, Item::Part(i), 0)).copied().unwrap_or(0.0);
                        let v = if threshold > 0.0 {
                            if p >= threshold as f64 { 1.0 } else { 0.0 }
                        } else {
                            p
                        };
                        changed |= self.set(path, Item::Part(i), 0, v);
                    }
                    OpticComponentKind::SignalIn => {
                        let k = iface.signal_in.iter().position(|&s| s == i).expect("listed");
                        changed |= self.set(path, Item::Part(i), 0, inputs.get(k).copied().unwrap_or(0.0));
                    }
                    _ => {}
                }
            }
            for inst in &layout.instances {
                let item = Item::Box(inst.id);
                let n_in = block_counts(&inst.block, self.docs).signal_in;
                let ins: Vec<f64> = (0..n_in).map(|k| self.value(path, layout, Terminal { item, port: k })).collect();
                let outs: Vec<f64> = match &inst.block {
                    BlockRef::Builtin(Builtin::NumberInput { bits, value }) => {
                        (0..*bits).rev().map(|k| ((value >> k) & 1) as f64).collect()
                    }
                    BlockRef::Builtin(Builtin::Value { value, .. }) => vec![*value],
                    BlockRef::Builtin(Builtin::Register { bits }) => self
                        .machine
                        .registers
                        .get(&(path.to_vec(), inst.id))
                        .cloned()
                        .unwrap_or_else(|| vec![0.0; *bits as usize]),
                    BlockRef::Builtin(cell @ Builtin::FunctionCell { .. }) => self.function_cell(cell, path, inst.id, &ins),
                    BlockRef::Builtin(Builtin::Model(m)) if m.script().is_some() && !m.signal_outputs.is_empty() => {
                        let wired: Vec<Option<f64>> = ins
                            .iter()
                            .enumerate()
                            .map(|(k, &v)| layout.signal_source(Terminal { item, port: k }).map(|_| v))
                            .collect();
                        match script_outputs(m, &wired, self.machine.scripts.get(&(path.to_vec(), inst.id))) {
                            Ok(outs) => outs,
                            Err(e) => {
                                self.error(format!("“{}”: {e}", m.name));
                                Vec::new()
                            }
                        }
                    }
                    BlockRef::Builtin(_) => Vec::new(),
                    BlockRef::Component(doc) => {
                        if self.stack.contains(doc) {
                            self.error(format!("“{}” contains itself", self.docs.name(*doc)));
                            Vec::new()
                        } else if let Some((child, _)) = self.docs.layout(*doc) {
                            let mut inner = path.to_vec();
                            inner.push(inst.id);
                            self.stack.push(*doc);
                            let outs = self.eval(child, &inner, &ins);
                            self.stack.pop();
                            outs
                        } else {
                            self.error("A component used here was deleted".into());
                            Vec::new()
                        }
                    }
                };
                for (k, v) in outs.into_iter().enumerate() {
                    changed |= self.set(path, item, k, v);
                }
            }
            if !changed {
                settled = true;
                break;
            }
        }
        if !settled {
            self.error("The signals did not settle; a feedback loop is oscillating".into());
        }
        iface
            .signal_out
            .iter()
            .map(|&i| self.value(path, layout, Terminal { item: Item::Part(i), port: 0 }))
            .collect()
    }

    fn function_cell(&mut self, cell: &Builtin, path: &[InstanceId], id: InstanceId, ins: &[f64]) -> Vec<f64> {
        let Builtin::FunctionCell {
            name,
            light_inputs,
            outputs,
            light_outputs,
            ..
        } = cell
        else {
            unreachable!()
        };
        let mut vars: HashMap<String, f64> = ins.iter().enumerate().map(|(k, v)| (format!("in{}", k + 1), *v)).collect();
        for k in 0..*light_inputs as usize {
            let p = self.powers.get(&Key::new(path, Item::Box(id), k)).copied().unwrap_or(0.0);
            vars.insert(format!("p{}", k + 1), p);
        }
        vars.insert("t".into(), self.machine.tick as f64);
        let run = |src: &str, what: String, errors: &mut Vec<String>| match expr::evaluate(src, &vars) {
            Ok(v) if v.is_finite() => v,
            Ok(_) => 0.0,
            Err(e) => {
                let message = format!("{name}, {what}: {e}");
                if !errors.contains(&message) {
                    errors.push(message);
                }
                0.0
            }
        };
        let mut errors = std::mem::take(&mut self.errors);
        let outs = outputs.iter().enumerate().map(|(k, src)| run(src, format!("out{}", k + 1), &mut errors)).collect();
        for (k, src) in light_outputs.iter().enumerate() {
            let v = run(src, format!("light{}", k + 1), &mut errors);
            self.light.insert(Key::new(path, Item::Box(id), k), v);
        }
        self.errors = errors;
        outs
    }
}

// ---- optics ------------------------------------------------------------------

struct Optics<'a> {
    docs: &'a dyn Documents,
    signals: &'a HashMap<Key, f64>,
    light: &'a HashMap<Key, f64>,
    max_len: f32,
    stack: Vec<DocId>,
    /// Nodes whose received power feeds the electronics.
    sensors: Vec<(Key, NodeId)>,
    /// Photonics source id for the next independent laser.
    next_source: u32,
    /// Display colour chosen for a source's light, by photonics source id.
    colors: HashMap<u32, [u8; 3]>,
}

/// Photonics source ids: shared sources A–H are 1–8; each independent laser
/// gets its own id from here up. Built-in laser blocks and function-cell
/// light share id 0.
const FIRST_INDEPENDENT_SOURCE: u32 = 1000;

/// A straight free-space path from one part (or box output) to the next.
pub struct Segment {
    pub start: Pos2,
    pub end: Pos2,
    pub dir: Vec2,
    pub input: InPort,
}

struct Built {
    ports: Ports,
    nodes: Vec<Option<NodeId>>,
    segments: Vec<Segment>,
    instance_ports: HashMap<InstanceId, Ports>,
    wire_sources: Vec<OutPort>,
}

enum Target {
    Part(usize),
    /// A box cell; light travelling right into an entry cell enters that optical input.
    Box { id: InstanceId, entry: Option<usize> },
}

impl Optics<'_> {
    /// Drive level of a primitive's signal input, if wired.
    fn drive(&self, layout: &Layout, path: &[InstanceId], part: usize) -> Option<f64> {
        self.drive_of(layout, path, Item::Part(part))
    }

    /// Drive level of an item's first signal input, if wired.
    fn drive_of(&self, layout: &Layout, path: &[InstanceId], item: Item) -> Option<f64> {
        self.drive_at(layout, path, item, 0)
    }

    /// Drive level of an item's signal input `port`, if wired.
    fn drive_at(&self, layout: &Layout, path: &[InstanceId], item: Item, port: usize) -> Option<f64> {
        layout
            .signal_source(Terminal { item, port })
            .map(|from| self.signals.get(&Key::new(path, from.item, from.port)).copied().unwrap_or(0.0))
    }

    fn build(&mut self, c: &mut Circuit, layout: &Layout, path: &[InstanceId], cell_um: f64) -> Result<Built, String> {
        let iface = interface(layout);
        // Primitives.
        let mut nodes = Vec::with_capacity(layout.components.len());
        let mut emits: Vec<Vec<(usize, usize)>> = Vec::with_capacity(layout.components.len());
        let mut accepts = Vec::with_capacity(layout.components.len());
        for (i, part) in layout.components.iter().enumerate() {
            let drive = self.drive(layout, path, i);
            let (node, emit, accept): (Option<NodeId>, Vec<(usize, usize)>, bool) = match part.kind {
                OpticComponentKind::Laser {
                    amplitude,
                    phase,
                    wavelength,
                    dir,
                    source,
                    color,
                } => {
                    let a = amplitude as f64 * drive.unwrap_or(1.0);
                    let id = if source == 0 {
                        self.next_source += 1;
                        self.next_source
                    } else {
                        source as u32
                    };
                    if let Some(color) = color {
                        self.colors.entry(id).or_insert(color);
                    }
                    let n = c.add(Laser::new(a, wavelength as f64).with_phase(phase as f64).with_source(id));
                    // A laser wired into a box shines there, not as a beam.
                    let beam = if layout.feed_of(i).is_some() { Vec::new() } else { vec![(0, direction_index(dir))] };
                    (Some(n), beam, false)
                }
                OpticComponentKind::Detector { .. } => {
                    let n = c.add(Detector::with_inputs(DIRECTIONS));
                    self.sensors.push((Key::new(path, Item::Part(i), 0), n));
                    (Some(n), Vec::new(), true)
                }
                OpticComponentKind::InputPort { dir } => {
                    let n = c.add(crate::photonics::PhaseShifter::new(0.0));
                    (Some(n), vec![(0, direction_index(dir))], false)
                }
                OpticComponentKind::OutputPort => {
                    let n = c.add(Custom::new("OutputPort", Matrix::full(1, DIRECTIONS, C64::ONE)));
                    (Some(n), Vec::new(), true)
                }
                OpticComponentKind::SignalIn | OpticComponentKind::SignalOut => (None, Vec::new(), false),
                kind => {
                    let kind = match (kind, drive) {
                        (OpticComponentKind::PhaseShifter { phase }, Some(v)) => OpticComponentKind::PhaseShifter {
                            phase: phase + std::f32::consts::PI * v as f32,
                        },
                        (OpticComponentKind::Attenuator { transmission }, Some(v)) => OpticComponentKind::Attenuator {
                            transmission: (transmission * v as f32).clamp(0.0, 1.0),
                        },
                        (k, _) => k,
                    };
                    let s = scattering(&kind).expect("passive part");
                    let emit = (0..DIRECTIONS)
                        .filter(|&k| (0..DIRECTIONS).any(|j| s[(k, j)] != C64::ZERO))
                        .map(|k| (k, k))
                        .collect();
                    (Some(c.add(Custom::new(kind.name(), s))), emit, true)
                }
            };
            nodes.push(node);
            emits.push(emit);
            accepts.push(accept);
        }

        // Boxes.
        let mut instance_ports = HashMap::new();
        let mut geometry = HashMap::new();
        for inst in &layout.instances {
            let ports = match &inst.block {
                #[cfg(not(target_arch = "wasm32"))]
                BlockRef::Builtin(Builtin::Model(m)) if m.script().is_some() => {
                    // A script's light can depend on its signals, as they are now.
                    let code = m.script().expect("checked");
                    let (engine, declared) = crate::panels::script::engine(code).map_err(|e| format!("“{}”: {e}", m.name))?;
                    let signals: Vec<Option<f64>> =
                        (0..declared.signal_inputs.len()).map(|k| self.drive_at(layout, path, Item::Box(inst.id), k)).collect();
                    let component = crate::panels::script::Scripted { name: m.name.clone(), engine, declared, signals };
                    // Its mistakes show now, as the simulation's error.
                    if !m.inputs.is_empty() && !m.outputs.is_empty() {
                        component.transmission(crate::photonics::REFERENCE_NM).map_err(|e| format!("“{}”: {e}", m.name))?;
                    }
                    let n = c.add(component);
                    Ports {
                        inputs: (0..m.inputs.len()).map(|k| n.input(k)).collect(),
                        outputs: (0..m.outputs.len()).map(|k| n.output(k)).collect(),
                    }
                }
                BlockRef::Builtin(b) => {
                    // A signal on a control input (attenuator, phase shifter) sets the block.
                    let driven = match b {
                        Builtin::Attenuator { .. } | Builtin::PhaseShifter { .. } => {
                            self.drive_of(layout, path, Item::Box(inst.id)).map(|v| b.driven(v))
                        }
                        _ => None,
                    };
                    let b = driven.as_ref().unwrap_or(b);
                    let light: Vec<f64> = (0..b.outputs().len())
                        .map(|k| self.light.get(&Key::new(path, Item::Box(inst.id), k)).copied().unwrap_or(0.0))
                        .collect();
                    let ports = b.build_with_light(c, &light).map_err(|e| e.to_string())?;
                    if matches!(b, Builtin::FunctionCell { .. }) {
                        for (k, input) in ports.inputs.iter().enumerate() {
                            self.sensors.push((Key::new(path, Item::Box(inst.id), k), input.node));
                        }
                    }
                    ports
                }
                BlockRef::Component(doc) => {
                    if self.stack.contains(doc) {
                        return Err(format!("“{}” contains itself", self.docs.name(*doc)));
                    }
                    let (child, child_cell) = self.docs.layout(*doc).ok_or("A component used here was deleted")?;
                    // This box's values for the component's exported parameters.
                    let configured = configure(child, &inst.params);
                    let child = configured.as_ref().unwrap_or(child);
                    let mut inner = path.to_vec();
                    inner.push(inst.id);
                    self.stack.push(*doc);
                    let built = self.build(c, child, &inner, child_cell);
                    self.stack.pop();
                    built.map_err(|e| {
                        if e.starts_with('“') || e.starts_with("A component") {
                            e
                        } else {
                            format!("In “{}”: {e}", self.docs.name(*doc))
                        }
                    })?
                    .ports
                }
            };
            let counts = PortCounts {
                optical_in: ports.inputs.len(),
                optical_out: ports.outputs.len(),
                ..block_counts(&inst.block, self.docs)
            };
            geometry.insert(inst.id, box_geometry(inst.cell, counts));
            instance_ports.insert(inst.id, ports);
        }

        // Waveguides between boxes.
        let mut wired_inputs = std::collections::HashSet::new();
        let mut wired_outputs = std::collections::HashSet::new();
        let mut wire_sources = Vec::with_capacity(layout.wires.len());
        for (k, wire) in layout.wires.iter().enumerate() {
            let from = instance_ports
                .get(&wire.from.instance)
                .and_then(|p| p.outputs.get(wire.from.port))
                .copied()
                .ok_or_else(|| format!("Waveguide {} starts at a port that no longer exists", k + 1))?;
            let to = instance_ports
                .get(&wire.to.instance)
                .and_then(|p| p.inputs.get(wire.to.port))
                .copied()
                .ok_or_else(|| format!("Waveguide {} ends at a port that no longer exists", k + 1))?;
            match wire.length_um.filter(|l| *l > 0.0) {
                Some(length) => {
                    let wg = c.add(Waveguide::new(length, 2.4));
                    c.connect(from, wg.input(0)).map_err(|e| e.to_string())?;
                    c.connect(wg.output(0), to).map_err(|e| e.to_string())?;
                }
                None => c.connect(from, to).map_err(|e| e.to_string())?,
            }
            wired_inputs.insert((wire.to.instance, wire.to.port));
            wired_outputs.insert((wire.from.instance, wire.from.port));
            wire_sources.push(from);
        }
        // Waveguides from lasers, numbered after the box-to-box ones.
        for feed in &layout.feeds {
            let from = nodes.get(feed.laser).copied().flatten().map(|n| n.output(0)).ok_or("A laser waveguide starts at a part that isn't a laser")?;
            let to = instance_ports
                .get(&feed.to.instance)
                .and_then(|p| p.inputs.get(feed.to.port))
                .copied()
                .ok_or("A laser waveguide ends at a port that no longer exists")?;
            c.connect(from, to).map_err(|e| e.to_string())?;
            wired_inputs.insert((feed.to.instance, feed.to.port));
            wire_sources.push(from);
        }

        // The layout's own light pins are its boundary: the light on them is
        // connected by whatever uses the layout, so no beam starts or ends
        // on them in here.
        let boundary = |items: &[Item]| -> std::collections::HashSet<InstanceId> {
            items.iter().filter_map(|i| if let Item::Box(id) = i { Some(*id) } else { None }).collect()
        };
        let (pins_in, pins_out) = (boundary(&iface.optical_in), boundary(&iface.optical_out));

        // Free-space beams: from emitting parts and from unwired box outputs.
        let mut targets: Vec<(Pos2, Target)> = layout
            .components
            .iter()
            .enumerate()
            .filter(|(_, p)| !matches!(p.kind, OpticComponentKind::SignalIn | OpticComponentKind::SignalOut))
            .map(|(i, p)| (p.pos, Target::Part(i)))
            .collect();
        for inst in &layout.instances {
            let g = &geometry[&inst.id];
            let inputs = instance_ports[&inst.id].inputs.len();
            for &(ci, cj) in &g.cells {
                let row = (cj - inst.cell.1) as usize;
                let entry = (ci == inst.cell.0 && row < inputs && !pins_in.contains(&inst.id)).then_some(row);
                targets.push((crate::panels::optics::model::cell_center(ci, cj), Target::Box { id: inst.id, entry }));
            }
        }

        let mut sources: Vec<(Pos2, OutPort, usize, Option<usize>)> = Vec::new();
        for (i, part) in layout.components.iter().enumerate() {
            if let Some(node) = nodes[i] {
                for &(port, k) in &emits[i] {
                    sources.push((part.pos, node.output(port), k, Some(i)));
                }
            }
        }
        for inst in layout.instances.iter().filter(|i| !pins_out.contains(&i.id)) {
            for (k, &out) in instance_ports[&inst.id].outputs.iter().enumerate() {
                if !wired_outputs.contains(&(inst.id, k)) {
                    sources.push((geometry[&inst.id].optical_out[k], out, 0, None));
                }
            }
        }

        let mut segments = Vec::new();
        for (start, from, k, own) in sources {
            let d = direction(k);
            let (dx, dy) = (d.x as f64, d.y as f64);
            let hit = targets
                .iter()
                .filter(|(_, t)| !matches!(t, Target::Part(j) if Some(*j) == own))
                .filter_map(|(pos, t)| {
                    let (wx, wy) = ((pos.x - start.x) as f64, (pos.y - start.y) as f64);
                    let along = wx * dx + wy * dy;
                    let across = (wx * dy - wy * dx).abs();
                    (along > 1.0 && across < 0.5).then_some((along, t, *pos))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let length = hit.as_ref().map_or(self.max_len as f64, |h| h.0);
            let path_node = c.add(Waveguide::free_space(length / GRID as f64 * cell_um));
            c.connect(from, path_node.input(0)).map_err(|e| e.to_string())?;
            if let Some((_, target, _)) = &hit {
                match target {
                    Target::Part(j) if accepts[*j] => {
                        let node = nodes[*j].expect("accepting parts have nodes");
                        c.connect(path_node.output(0), node.input(k)).map_err(|e| e.to_string())?;
                    }
                    Target::Box { id, entry: Some(row) } if k == 0 && !wired_inputs.contains(&(*id, *row)) => {
                        let to = instance_ports[id].inputs[*row];
                        c.connect(path_node.output(0), to).map_err(|e| e.to_string())?;
                    }
                    _ => {}
                }
            }
            let end = match &hit {
                Some((_, Target::Box { .. }, pos)) => {
                    // Stop at the box edge rather than its cell centre.
                    *pos - d * (GRID / 2.0)
                }
                Some((_, _, pos)) => *pos,
                None => start + d * self.max_len,
            };
            segments.push(Segment {
                start,
                end,
                dir: d,
                input: path_node.input(0),
            });
        }

        // The layout's own boundary.
        let endpoint_in = |item: &Item| match item {
            Item::Part(i) => nodes[*i].expect("port part").input(0),
            Item::Box(id) => instance_ports[id].inputs[0],
        };
        let endpoint_out = |item: &Item| match item {
            Item::Part(i) => nodes[*i].expect("port part").output(0),
            Item::Box(id) => instance_ports[id].outputs[0],
        };
        Ok(Built {
            ports: Ports {
                inputs: iface.optical_in.iter().map(endpoint_in).collect(),
                outputs: iface.optical_out.iter().map(endpoint_out).collect(),
            },
            nodes,
            segments,
            instance_ports,
            wire_sources,
        })
    }
}

// ---- top level -------------------------------------------------------------------

/// A component's layout with an instance's values for its exported
/// parameters applied, or `None` when the instance sets none (use it as is).
pub fn configure(component: &Layout, values: &[(String, f64)]) -> Option<Layout> {
    if values.is_empty() {
        return None;
    }
    let mut copy = component.clone();
    for (name, value) in values {
        if let Some(export) = component.exports.iter().find(|e| &e.name == name) {
            super::sweep::set(&mut copy, export.item, &export.param, *value);
        }
    }
    Some(copy)
}

fn same(a: &HashMap<Key, f64>, b: &HashMap<Key, f64>, tolerance: f64) -> bool {
    a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| (v - w).abs() <= tolerance))
}

/// Settle the layout's optics and electronics. `root` is the document being
/// simulated, so it can't be placed inside itself.
pub fn simulate(
    layout: &Layout,
    cell_um: f64,
    docs: &dyn Documents,
    machine: &Machine,
    root: Option<DocId>,
    options: &Options,
) -> Outcome {
    let settled = match settle(layout, cell_um, docs, machine, root, options) {
        Ok(s) => s,
        Err(e) => return Outcome::failed(e),
    };
    let Settled { solutions, built, source_colors, iteration, stable, signals, .. } = settled;
    outcome_of(layout, solutions, built, source_colors, iteration, stable, signals)
}

/// How light passes between a layout's light ports, at each of several
/// wavelengths, with its own lasers off: `matrices[k][(o, i)]` is the field
/// leaving output `o` per unit field entering input `i` at
/// `wavelengths_nm[k]`. Signals inside (phase shifters, switches) are at the
/// values they settle to, as in [`simulate`].
#[derive(Clone, Debug)]
pub struct Transmission {
    pub wavelengths_nm: Vec<f64>,
    pub matrices: Vec<crate::photonics::Matrix>,
}

pub fn transmission(
    layout: &Layout,
    cell_um: f64,
    docs: &dyn Documents,
    machine: &Machine,
    root: Option<DocId>,
    wavelengths_nm: &[f64],
) -> Result<Transmission, String> {
    let settled = settle(layout, cell_um, docs, machine, root, &Options::default())?;
    if !settled.stable {
        return Err("The circuit did not settle; a feedback loop may be oscillating".into());
    }
    let ports = &settled.built.ports;
    if ports.inputs.is_empty() || ports.outputs.is_empty() {
        return Err("It needs light Input and Output ports (or pins) to have S-parameters".into());
    }
    let circuit = &settled.circuit;
    let matrices = crate::photonics::par::try_map(wavelengths_nm, |&wl| circuit.transfer_matrix(wl, &ports.inputs, &ports.outputs))
        .map_err(|e| e.to_string())?;
    Ok(Transmission { wavelengths_nm: wavelengths_nm.to_vec(), matrices })
}

/// The circuit a layout simulates as, with signals inside at the values they
/// settle to, and the circuit's ends for the layout's light inputs and
/// outputs: what netlists for other simulators are written from.
pub fn circuit(
    layout: &Layout,
    cell_um: f64,
    docs: &dyn Documents,
    machine: &Machine,
    root: Option<DocId>,
) -> Result<(Circuit, crate::photonics::Ports), String> {
    let settled = settle(layout, cell_um, docs, machine, root, &Options::default())?;
    if !settled.stable {
        return Err("The circuit did not settle; a feedback loop may be oscillating".into());
    }
    Ok((settled.circuit, settled.built.ports))
}

/// The circuit of a layout once its electronics and light agree, with the
/// light of its sources.
struct Settled {
    circuit: Circuit,
    solutions: Vec<crate::photonics::Solution>,
    built: Built,
    source_colors: HashMap<u32, [u8; 3]>,
    iteration: usize,
    stable: bool,
    signals: HashMap<Key, f64>,
}

fn settle(
    layout: &Layout,
    cell_um: f64,
    docs: &dyn Documents,
    machine: &Machine,
    root: Option<DocId>,
    options: &Options,
) -> Result<Settled, String> {
    let mut signals: HashMap<Key, f64> = HashMap::new();
    let mut powers: HashMap<Key, f64> = HashMap::new();
    // The last round's light, turned into an outcome once it settles: the
    // beams and fields are only needed for the final state.
    let mut last = None;
    for iteration in 1..=options.max_iterations.max(2) {
        let mut electronics = Electronics {
            docs,
            machine,
            powers: &powers,
            signals: signals.clone(),
            light: HashMap::new(),
            stack: root.into_iter().collect(),
            errors: Vec::new(),
        };
        electronics.eval(layout, &[], &machine.inputs);
        if let Some(e) = electronics.errors.first() {
            return Err(e.clone());
        }

        let mut optics = Optics {
            docs,
            signals: &electronics.signals,
            light: &electronics.light,
            max_len: options.max_len,
            stack: root.into_iter().collect(),
            sensors: Vec::new(),
            next_source: FIRST_INDEPENDENT_SOURCE,
            colors: HashMap::new(),
        };
        let mut circuit = Circuit::new();
        let built = optics.build(&mut circuit, layout, &[], cell_um)?;
        let solutions = circuit.simulate_sources().map_err(|e| e.to_string())?;
        let total = |f: &dyn Fn(&crate::photonics::Solution) -> f64| solutions.iter().map(f).fold(0.0, |a, b| a + b);
        let new_powers: HashMap<Key, f64> = optics
            .sensors
            .iter()
            .map(|(key, node)| (key.clone(), total(&|s| s.detected_power(*node))))
            .collect();

        let source_colors = std::mem::take(&mut optics.colors);
        let stable = iteration > 1 && same(&electronics.signals, &signals, 1e-12) && same(&new_powers, &powers, 1e-9);
        signals = electronics.signals;
        powers = new_powers;

        last = Some(Settled { circuit, solutions, built, source_colors, iteration, stable, signals: signals.clone() });
        if stable {
            break;
        }
    }
    last.ok_or_else(|| "nothing to simulate".to_owned())
}

#[allow(clippy::too_many_arguments)]
fn outcome_of(
    layout: &Layout,
    solutions: Vec<crate::photonics::Solution>,
    built: Built,
    source_colors: HashMap<u32, [u8; 3]>,
    iteration: usize,
    stable: bool,
    signals: HashMap<Key, f64>,
) -> Outcome {
    let total = |f: &dyn Fn(&crate::photonics::Solution) -> f64| solutions.iter().map(f).fold(0.0, |a, b| a + b);
        let colors: Vec<Option<[u8; 3]>> =
            solutions.iter().map(|s| s.source().and_then(|id| source_colors.get(&id).copied())).collect();
        let mut outcome = Outcome {
            wavelengths: solutions.iter().map(|s| s.wavelength_nm()).collect(),
            channel_colors: colors.clone(),
            beams: solutions
                .iter()
                .zip(&colors)
                .flat_map(|(sol, &color)| {
                    built.segments.iter().filter_map(move |seg| {
                        let field = sol.input_field(seg.input);
                        (field.norm_sqr() > 1e-12).then(|| Beam {
                            start: seg.start,
                            end: seg.end,
                            dir: seg.dir,
                            wavelength_nm: sol.wavelength_nm(),
                            field,
                            color,
                        })
                    })
                })
                .collect(),
            detector_power: layout
                .components
                .iter()
                .enumerate()
                .filter_map(|(i, part)| {
                    let node = built.nodes[i]?;
                    match part.kind {
                        OpticComponentKind::Detector { .. } => Some((i, total(&|s| s.detected_power(node)))),
                        OpticComponentKind::OutputPort => Some((i, total(&|s| s.output_field(node.output(0)).norm_sqr()))),
                        _ => None,
                    }
                })
                .collect(),
            wire_fields: built
                .wire_sources
                .iter()
                .map(|&from| solutions.iter().map(|s| s.output_field(from)).collect())
                .collect(),
            port_power: built
                .instance_ports
                .iter()
                .flat_map(|(id, ports)| {
                    ports
                        .outputs
                        .iter()
                        .enumerate()
                        .map(|(k, &out)| ((*id, k), total(&|s| s.output_field(out).norm_sqr())))
                        .collect::<Vec<_>>()
                })
                .collect(),
            signals: signals.clone(),
            iterations: iteration,
            converged: stable,
            error: None,
        };
    if !outcome.converged && outcome.error.is_none() {
        outcome.error = Some(format!(
            "The circuit did not settle after {} rounds; a feedback loop may be oscillating",
            outcome.iterations
        ));
    }
    outcome
}

/// Advance the clock: every register stores its data inputs (when its write
/// input is high or unconnected), using the signals of a settled `outcome`.
pub fn clock(layout: &Layout, docs: &dyn Documents, machine: &mut Machine, outcome: &Outcome) {
    #[allow(clippy::too_many_arguments)]
    fn walk(
        layout: &Layout,
        path: &[InstanceId],
        docs: &dyn Documents,
        signals: &HashMap<Key, f64>,
        out: &mut HashMap<(Path, InstanceId), Vec<f64>>,
        scripts: &HashMap<(Path, InstanceId), String>,
        script_updates: &mut HashMap<(Path, InstanceId), String>,
        depth: usize,
    ) {
        if depth > 32 {
            return;
        }
        let input = |item: Item, port: usize| {
            layout
                .signal_source(Terminal { item, port })
                .map(|from| signals.get(&Key::new(path, from.item, from.port)).copied().unwrap_or(0.0))
        };
        for inst in &layout.instances {
            match &inst.block {
                BlockRef::Builtin(Builtin::Register { bits }) => {
                    let item = Item::Box(inst.id);
                    if input(item, *bits as usize).is_none_or(|w| w >= 0.5) {
                        let stored = (0..*bits as usize)
                            .map(|k| if input(item, k).unwrap_or(0.0) >= 0.5 { 1.0 } else { 0.0 })
                            .collect();
                        out.insert((path.to_vec(), inst.id), stored);
                    }
                }
                BlockRef::Component(doc) => {
                    if let Some((child, _)) = docs.layout(*doc) {
                        let mut inner = path.to_vec();
                        inner.push(inst.id);
                        walk(child, &inner, docs, signals, out, scripts, script_updates, depth + 1);
                    }
                }
                BlockRef::Builtin(Builtin::Model(m)) if m.script().is_some() => {
                    let item = Item::Box(inst.id);
                    let signals: Vec<Option<f64>> = (0..m.signal_inputs.len()).map(|k| input(item, k)).collect();
                    let key = (path.to_vec(), inst.id);
                    match script_tick(m, &signals, scripts.get(&key)) {
                        Ok(next) => {
                            script_updates.insert(key, next);
                        }
                        Err(e) => tracing::warn!("“{}” on a clock tick: {e}", m.name),
                    }
                }
                BlockRef::Builtin(_) => {}
            }
        }
    }
    let mut updates = HashMap::new();
    let mut script_updates = HashMap::new();
    walk(layout, &[], docs, &outcome.signals, &mut updates, &machine.scripts, &mut script_updates, 0);
    machine.registers.extend(updates);
    machine.scripts.extend(script_updates);
    machine.tick += 1;
}

/// A scripted component's signal outputs.
#[cfg(not(target_arch = "wasm32"))]
fn script_outputs(m: &crate::panels::models::ModelSpec, signals: &[Option<f64>], state: Option<&String>) -> Result<Vec<f64>, String> {
    let (engine, declared) = crate::panels::script::engine(m.script().unwrap_or_default())?;
    let state = state.cloned().unwrap_or_else(|| declared.state.clone());
    let e = engine.lock().map_err(|_| "script engine poisoned")?;
    e.outputs(&declared, signals, &state)
}

/// A scripted component's next state on a clock tick.
#[cfg(not(target_arch = "wasm32"))]
fn script_tick(m: &crate::panels::models::ModelSpec, signals: &[Option<f64>], state: Option<&String>) -> Result<String, String> {
    let (engine, declared) = crate::panels::script::engine(m.script().unwrap_or_default())?;
    let state = state.cloned().unwrap_or_else(|| declared.state.clone());
    let e = engine.lock().map_err(|_| "script engine poisoned")?;
    e.tick(&declared, signals, &state)
}

#[cfg(target_arch = "wasm32")]
fn script_outputs(_: &crate::panels::models::ModelSpec, _: &[Option<f64>], _: Option<&String>) -> Result<Vec<f64>, String> {
    Err("scripted components run in the desktop app".into())
}

#[cfg(target_arch = "wasm32")]
fn script_tick(_: &crate::panels::models::ModelSpec, _: &[Option<f64>], _: Option<&String>) -> Result<String, String> {
    Err("scripted components run in the desktop app".into())
}

/// Read a number from bits listed most significant first.
pub fn bits_to_number(bits: &[f64]) -> u64 {
    bits.iter().fold(0, |acc, &b| (acc << 1) | u64::from(b >= 0.5))
}

#[cfg(test)]
mod tests;
