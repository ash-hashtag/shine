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

use egui::{Pos2, Vec2};

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
    pub wavelengths: Vec<f64>,
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
                    BlockRef::Builtin(Builtin::Register { bits }) => self
                        .machine
                        .registers
                        .get(&(path.to_vec(), inst.id))
                        .cloned()
                        .unwrap_or_else(|| vec![0.0; *bits as usize]),
                    BlockRef::Builtin(cell @ Builtin::FunctionCell { .. }) => self.function_cell(cell, path, inst.id, &ins),
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
}

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
        layout
            .signal_source(Terminal { item: Item::Part(part), port: 0 })
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
                } => {
                    let a = amplitude as f64 * drive.unwrap_or(1.0);
                    let n = c.add(Laser::new(a, wavelength as f64).with_phase(phase as f64));
                    (Some(n), vec![(0, direction_index(dir))], false)
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
                BlockRef::Builtin(b) => {
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
                let entry = (ci == inst.cell.0 && row < inputs).then_some(row);
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
        for inst in &layout.instances {
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
    let mut signals: HashMap<Key, f64> = HashMap::new();
    let mut powers: HashMap<Key, f64> = HashMap::new();
    let mut outcome = Outcome::default();
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
            return Outcome::failed(e.clone());
        }

        let mut optics = Optics {
            docs,
            signals: &electronics.signals,
            light: &electronics.light,
            max_len: options.max_len,
            stack: root.into_iter().collect(),
            sensors: Vec::new(),
        };
        let mut circuit = Circuit::new();
        let built = match optics.build(&mut circuit, layout, &[], cell_um) {
            Ok(b) => b,
            Err(e) => return Outcome::failed(e),
        };
        let solutions = match circuit.simulate_sources() {
            Ok(s) => s,
            Err(e) => return Outcome::failed(e.to_string()),
        };
        let total = |f: &dyn Fn(&crate::photonics::Solution) -> f64| solutions.iter().map(f).fold(0.0, |a, b| a + b);
        let new_powers: HashMap<Key, f64> = optics
            .sensors
            .iter()
            .map(|(key, node)| (key.clone(), total(&|s| s.detected_power(*node))))
            .collect();

        let stable = iteration > 1 && same(&electronics.signals, &signals, 1e-12) && same(&new_powers, &powers, 1e-9);
        signals = electronics.signals;
        powers = new_powers;

        outcome = Outcome {
            wavelengths: solutions.iter().map(|s| s.wavelength_nm()).collect(),
            beams: solutions
                .iter()
                .flat_map(|sol| {
                    built.segments.iter().filter_map(move |seg| {
                        let field = sol.input_field(seg.input);
                        (field.norm_sqr() > 1e-12).then(|| Beam {
                            start: seg.start,
                            end: seg.end,
                            dir: seg.dir,
                            wavelength_nm: sol.wavelength_nm(),
                            field,
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
        if stable {
            break;
        }
    }
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
    fn walk(layout: &Layout, path: &[InstanceId], docs: &dyn Documents, signals: &HashMap<Key, f64>, out: &mut HashMap<(Path, InstanceId), Vec<f64>>, depth: usize) {
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
                        walk(child, &inner, docs, signals, out, depth + 1);
                    }
                }
                BlockRef::Builtin(_) => {}
            }
        }
    }
    let mut updates = HashMap::new();
    walk(layout, &[], docs, &outcome.signals, &mut updates, 0);
    machine.registers.extend(updates);
    machine.tick += 1;
}

/// Read a number from bits listed most significant first.
pub fn bits_to_number(bits: &[f64]) -> u64 {
    bits.iter().fold(0, |acc, &b| (acc << 1) | u64::from(b >= 0.5))
}

#[cfg(test)]
mod tests;
