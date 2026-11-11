//! A layout description for [gdsfactory](https://gdsfactory.github.io): its
//! YAML "PIC" format of instances, placements, routes and ports. gdsfactory
//! turns it into a mask layout (GDS or OASIS) with a foundry's process kit,
//! so this is the step from a Shine circuit towards a chip that can be made.
//!
//! Each waveguide block becomes the matching component of gdsfactory's
//! generic kit (couplers, MMIs, rings, heaters, crossings), placed where it
//! sits on Shine's grid; every waveguide becomes a route that gdsfactory
//! draws. Components used inside a design are flattened into it. Light pins,
//! lasers and detectors become the chip's ports. Free-space parts and the
//! electronics have no place on a photonic chip and are left out, with a note.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::fmt::Write as _;

use crate::panels::circuit::sim;
use crate::panels::optics::model::{BlockRef, Builtin, Item, Layout, OpticComponentKind, cell_of};
use crate::panels::project::{DocId, Project};

/// Room left between neighbouring components (and for empty grid columns
/// and rows), in µm, when nothing else is asked for.
pub const DEFAULT_SPACING_UM: f64 = 40.0;

/// The result: the YAML text, and what didn't carry over.
#[derive(Clone, Debug, PartialEq)]
pub struct PicExport {
    pub yaml: String,
    pub instances: usize,
    pub routes: usize,
    pub ports: usize,
    pub notes: Vec<String>,
}

/// An instance of a gdsfactory component.
struct Instance {
    name: String,
    component: &'static str,
    settings: Vec<(&'static str, f64)>,
    x: f64,
    y: f64,
    rotation: i32,
    /// What Shine had, as a comment (values gdsfactory can't take directly).
    note: Option<String>,
}

/// One end of a connection, as a box's light port resolves to.
#[derive(Clone, Debug, PartialEq)]
enum End {
    /// A port of a gdsfactory instance.
    Port { inst: String, port: String },
    /// The layout's own light input `k` (an Input pin), on its output side.
    InputPin(usize),
    /// The layout's own light output `k` (an Output pin), on its input side.
    OutputPin(usize),
    /// Light enters the chip here (a laser): a port named so.
    Source(String),
    /// Light leaves the chip here (a detector): a port named so.
    Sink(String),
}

fn port(inst: &str, port: &str) -> Option<End> {
    Some(End::Port { inst: inst.to_owned(), port: port.to_owned() })
}

/// Where light enters (`ins`) and leaves (`outs`) a box.
#[derive(Default)]
struct BoxEnds {
    ins: Vec<Option<End>>,
    outs: Vec<Option<End>>,
}

/// The ends of a layout's own light inputs and outputs, once flattened.
#[derive(Default)]
struct Interface {
    inputs: Vec<Option<End>>,
    outputs: Vec<Option<End>>,
}

struct Builder<'a> {
    project: &'a Project,
    instances: Vec<Instance>,
    links: Vec<(End, End)>,
    /// The chip's ports: name and the instance port.
    ports: Vec<(String, End)>,
    notes: Vec<String>,
    /// The size each component takes once laid out, by document.
    sizes: HashMap<DocId, (f64, f64)>,
    spacing: f64,
}

/// Where a layout's grid columns and rows go, in µm from its corner: each is
/// as wide (tall) as the largest thing starting in it, plus the spacing.
struct Grid {
    xs: HashMap<i32, f64>,
    ys: HashMap<i32, f64>,
    size: (f64, f64),
}

impl Grid {
    fn at(&self, cell: (i32, i32)) -> (f64, f64) {
        (self.xs.get(&cell.0).copied().unwrap_or(0.0), -self.ys.get(&cell.1).copied().unwrap_or(0.0))
    }
}

/// About how much room a gdsfactory component of the generic kit takes
/// (width, height in µm), from its sizes there.
fn footprint(b: &Builtin) -> (f64, f64) {
    match b {
        Builtin::InputPin { .. } | Builtin::OutputPin { .. } | Builtin::Laser { .. } | Builtin::Detector => (0.0, 0.0),
        Builtin::Coupler { .. } => (40.0, 6.0),
        Builtin::YSplitter | Builtin::YCombiner => (26.0, 3.0),
        Builtin::Crossing => (8.0, 8.0),
        Builtin::PhaseShifter { .. } => (352.0, 11.0),
        Builtin::Attenuator { .. } => (10.0, 1.0),
        Builtin::DelayLine { length_um, .. } => (length_um.max(0.1), 1.0),
        Builtin::Mzi { .. } => (291.0, 56.0),
        Builtin::Ring { length_um, .. } | Builtin::AddDropRing { length_um, .. } => {
            let r = (length_um / (2.0 * PI)).max(5.0);
            (2.0 * r + 20.0, 2.0 * r + 12.0)
        }
        Builtin::SplitterTree { levels } => (*levels as f64 * 60.0 + 26.0, 60.0 * (1u32 << levels) as f64),
        Builtin::SwitchTree { levels, .. } => (*levels as f64 * 400.0 + 291.0, 60.0 * (1u32 << levels) as f64),
        _ => (0.0, 0.0),
    }
}

fn coord(v: i32) -> String {
    if v < 0 { format!("m{}", -v) } else { v.to_string() }
}

/// A name gdsfactory accepts: letters, digits and underscores.
fn ident(text: &str) -> String {
    let s: String = text.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    let s = s.trim_matches('_').to_owned();
    let mut collapsed = String::new();
    for c in s.chars() {
        if !(c == '_' && collapsed.ends_with('_')) {
            collapsed.push(c);
        }
    }
    if collapsed.is_empty() || collapsed.starts_with(|c: char| c.is_ascii_digit()) { format!("c_{collapsed}") } else { collapsed }
}

impl Builder<'_> {
    fn note(&mut self, text: String) {
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

    /// The room each block of `layout` takes, laid out.
    fn size_of(&mut self, inst: &crate::panels::optics::model::Instance, depth: usize) -> (f64, f64) {
        match &inst.block {
            BlockRef::Builtin(b) => footprint(b),
            BlockRef::Component(doc) => {
                if let Some(&s) = self.sizes.get(doc) {
                    return s;
                }
                let Some(child) = self.project.doc(*doc) else { return (0.0, 0.0) };
                let s = if depth > 16 { (0.0, 0.0) } else { self.grid(child.layout(), depth + 1).size };
                self.sizes.insert(*doc, s);
                s
            }
        }
    }

    /// Where `layout`'s columns and rows go, keeping its blocks' order and
    /// arrangement with room for each.
    fn grid(&mut self, layout: &Layout, depth: usize) -> Grid {
        let mut widths: HashMap<i32, f64> = HashMap::new();
        let mut heights: HashMap<i32, f64> = HashMap::new();
        for inst in &layout.instances {
            let (w, h) = self.size_of(inst, depth);
            let cw = widths.entry(inst.cell.0).or_default();
            *cw = cw.max(w);
            let ch = heights.entry(inst.cell.1).or_default();
            *ch = ch.max(h);
        }
        let place = |sizes: &HashMap<i32, f64>, spacing: f64| -> (HashMap<i32, f64>, f64) {
            let (Some(&lo), Some(&hi)) = (sizes.keys().min(), sizes.keys().max()) else { return (HashMap::new(), 0.0) };
            let mut at = HashMap::new();
            let mut pos = 0.0;
            for c in lo..=hi {
                at.insert(c, pos);
                // An empty column still leaves a little room, for waveguides.
                pos += sizes.get(&c).map_or(spacing / 2.0, |s| s + spacing);
            }
            (at, pos)
        };
        let (xs, w) = place(&widths, self.spacing);
        let (ys, h) = place(&heights, self.spacing);
        Grid { xs, ys, size: (w, h) }
    }

    /// Add `layout`'s blocks (with `prefix` on their names), at `origin` µm.
    fn flatten(&mut self, layout: &Layout, prefix: &str, origin: (f64, f64), depth: usize, top: bool) -> Interface {
        let grid = self.grid(layout, depth);
        let iface = sim::interface(layout);
        let pin_index = |items: &[Item], id| items.iter().position(|it| *it == Item::Box(id));
        let mut boxes: HashMap<_, BoxEnds> = HashMap::new();
        for inst in &layout.instances {
            let (dx, dy) = grid.at(inst.cell);
            let at = (origin.0 + dx, origin.1 + dy);
            let name = |kind: &str| format!("{prefix}{kind}_{}_{}", coord(inst.cell.0), coord(inst.cell.1));
            let ends = match &inst.block {
                BlockRef::Builtin(b) => self.block(b, &name(&ident(b.info().name)), at, pin_index(&iface.optical_in, inst.id), pin_index(&iface.optical_out, inst.id), top),
                BlockRef::Component(doc) => {
                    let Some(child) = self.project.doc(*doc) else {
                        self.note("A component used here was deleted; its box was left out".into());
                        continue;
                    };
                    if depth > 16 {
                        self.note("Components nested too deeply were left out".into());
                        continue;
                    }
                    let child_prefix = format!("{}__", name(&ident(&child.name)));
                    let inner = self.flatten(child.layout(), &child_prefix, at, depth + 1, false);
                    BoxEnds { ins: inner.inputs, outs: inner.outputs }
                }
            };
            boxes.insert(inst.id, ends);
        }

        let mut out = Interface { inputs: vec![None; iface.optical_in.len()], outputs: vec![None; iface.optical_out.len()] };
        let mut connect = |b: &mut Self, from: Option<End>, to: Option<End>| match (from, to) {
            (Some(f @ End::Port { .. }), Some(t @ End::Port { .. })) => b.links.push((f, t)),
            (Some(End::InputPin(k)), Some(t @ End::Port { .. })) => out.inputs[k] = Some(t),
            (Some(f @ End::Port { .. }), Some(End::OutputPin(k))) => out.outputs[k] = Some(f),
            (Some(End::Source(name)), Some(t @ End::Port { .. })) => b.ports.push((name, t)),
            (Some(f @ End::Port { .. }), Some(End::Sink(name))) => b.ports.push((name, f)),
            (None, _) | (_, None) => {}
            _ => b.note("A waveguide straight from a pin, laser or detector to another was left out".into()),
        };
        for w in &layout.wires {
            let from = boxes.get(&w.from.instance).and_then(|e| e.outs.get(w.from.port).cloned().flatten());
            let to = boxes.get(&w.to.instance).and_then(|e| e.ins.get(w.to.port).cloned().flatten());
            connect(self, from, to);
        }
        // Lasers wired into boxes: where light enters the chip.
        for f in &layout.feeds {
            let to = boxes.get(&f.to.instance).and_then(|e| e.ins.get(f.to.port).cloned().flatten());
            let (x, y) = layout.components.get(f.laser).map_or((0, 0), |c| cell_of(c.pos));
            let name = format!("{prefix}laser_{}_{}", coord(x), coord(y));
            connect(self, Some(End::Source(name)), to);
        }
        let free_space = layout
            .components
            .iter()
            .enumerate()
            .filter(|(i, c)| match c.kind {
                OpticComponentKind::Laser { .. } => !layout.feeds.iter().any(|f| f.laser == *i),
                OpticComponentKind::SignalIn | OpticComponentKind::SignalOut => false,
                _ => true,
            })
            .count();
        if free_space > 0 {
            self.note(format!(
                "{free_space} free-space part(s){} aren't on a chip and were left out",
                if prefix.is_empty() { String::new() } else { format!(" in {}", prefix.trim_end_matches('_')) }
            ));
        }
        if !layout.signals.is_empty() {
            self.note("Signal wires and electronics aren't part of the photonic layout: wire heaters' electrical ports in gdsfactory".into());
        }
        out
    }

    fn add(&mut self, name: &str, component: &'static str, settings: Vec<(&'static str, f64)>, at: (f64, f64), rotation: i32, note: Option<String>) {
        self.instances.push(Instance { name: name.to_owned(), component, settings, x: at.0, y: at.1, rotation, note });
    }

    /// A built-in block as gdsfactory components; its light ports' ends.
    fn block(&mut self, b: &Builtin, name: &str, at: (f64, f64), input_pin: Option<usize>, output_pin: Option<usize>, top: bool) -> BoxEnds {
        let two = |n: &str| BoxEnds { ins: vec![port(n, "o2"), port(n, "o1")], outs: vec![port(n, "o3"), port(n, "o4")] };
        let through = |n: &str| BoxEnds { ins: vec![port(n, "o1")], outs: vec![port(n, "o2")] };
        match b {
            Builtin::InputPin { name: pin } => {
                let k = input_pin.unwrap_or(0);
                // At the top, the pin's waveguide starts at a chip port named after it.
                BoxEnds { ins: Vec::new(), outs: vec![Some(if top { End::Source(ident(pin)) } else { End::InputPin(k) })] }
            }
            Builtin::OutputPin { name: pin } => {
                let k = output_pin.unwrap_or(0);
                BoxEnds { ins: vec![Some(if top { End::Sink(ident(pin)) } else { End::OutputPin(k) })], outs: Vec::new() }
            }
            Builtin::Laser { .. } => BoxEnds { ins: Vec::new(), outs: vec![Some(End::Source(name.to_owned()))] },
            Builtin::Detector => BoxEnds { ins: vec![Some(End::Sink(name.to_owned()))], outs: Vec::new() },
            Builtin::Coupler { coupling } => {
                if (coupling - 0.5).abs() < 1e-9 {
                    self.add(name, "mmi2x2", Vec::new(), at, 0, Some("Shine: 50:50 coupler".into()));
                } else {
                    self.add(name, "coupler", Vec::new(), at, 0, Some(format!("Shine: coupling {coupling} to the cross port; set gap and length for it in your PDK")));
                }
                two(name)
            }
            Builtin::Mzi { theta, phi } => {
                self.add(name, "mzi2x2_2x2_phase_shifter", Vec::new(), at, 0, Some(format!("Shine: θ = {theta:.4} rad, φ = {phi:.4} rad, set by its heater")));
                two(name)
            }
            Builtin::PhaseShifter { phase } => {
                self.add(name, "straight_heater_metal", Vec::new(), at, 0, Some(format!("Shine: phase {phase:.4} rad, set by the heater")));
                through(name)
            }
            Builtin::Attenuator { loss_db } => {
                self.add(name, "straight", vec![("length", 10.0)], at, 0, Some(format!("Shine: {loss_db} dB loss; use your PDK's attenuator or a doped section")));
                through(name)
            }
            Builtin::DelayLine { length_um, n_eff, n_g, loss_db_per_cm, .. } => {
                self.add(
                    name,
                    "straight",
                    vec![("length", length_um.max(0.1))],
                    at,
                    0,
                    Some(format!("Shine: n_eff {n_eff}, n_g {n_g}, {loss_db_per_cm} dB/cm; a long delay is better as a spiral")),
                );
                through(name)
            }
            Builtin::Ring { coupling, length_um, .. } | Builtin::AddDropRing { coupling, length_um, .. } => {
                let radius = length_um / (2.0 * PI);
                let min = 5.0;
                if radius < min {
                    self.note(format!(
                        "Rings shorter than {} µm round would bend tighter than a {min} µm radius; they're drawn with a {min} µm radius (a longer ring, so a smaller FSR)",
                        number(2.0 * PI * min)
                    ));
                }
                let settings = vec![("radius", radius.max(min)), ("length_x", 0.0)];
                let note = Some(format!("Shine: {} µm round, coupling {coupling}; set the gap for it in your PDK", number(*length_um)));
                if matches!(b, Builtin::Ring { .. }) {
                    self.add(name, "ring_single", settings, at, 0, note);
                    through(name)
                } else {
                    self.add(name, "ring_double", settings, at, 0, note);
                    // Through on the bus; the drop leaves the far bus backwards.
                    BoxEnds { ins: vec![port(name, "o1"), port(name, "o4")], outs: vec![port(name, "o2"), port(name, "o3")] }
                }
            }
            Builtin::YSplitter => {
                self.add(name, "mmi1x2", Vec::new(), at, 0, None);
                BoxEnds { ins: vec![port(name, "o1")], outs: vec![port(name, "o2"), port(name, "o3")] }
            }
            Builtin::YCombiner => {
                // A splitter turned round: its upper arm is now below.
                self.add(name, "mmi1x2", Vec::new(), at, 180, Some("Shine: Y-combiner".into()));
                BoxEnds { ins: vec![port(name, "o3"), port(name, "o2")], outs: vec![port(name, "o1")] }
            }
            Builtin::Crossing => {
                self.add(name, "crossing", Vec::new(), at, 0, None);
                // in1 → out2 and in2 → out1, as the crossing's straight paths.
                BoxEnds { ins: vec![port(name, "o1"), port(name, "o4")], outs: vec![port(name, "o2"), port(name, "o3")] }
            }
            Builtin::SplitterTree { levels } | Builtin::SwitchTree { levels, .. } => {
                let switch = matches!(b, Builtin::SwitchTree { .. });
                self.tree(name, *levels, switch, at)
            }
            Builtin::Dft { n } => {
                self.note(format!("The {n}-mode optical DFT has no gdsfactory component; build it from couplers and phase shifters"));
                BoxEnds { ins: vec![None; *n as usize], outs: vec![None; *n as usize] }
            }
            Builtin::FunctionCell { .. } => {
                self.note("Function cells are electronics and were left out".into());
                BoxEnds { ins: vec![None; b.inputs().len()], outs: vec![None; b.outputs().len()] }
            }
            Builtin::NumberInput { .. } | Builtin::Display { .. } | Builtin::Register { .. } => BoxEnds::default(),
            Builtin::Model(m) => {
                let by = if m.script().is_some() { "a script" } else { "its S-parameters" };
                self.note(format!("“{}” is defined by {by}, not a shape: place your kit's cell for it", m.name));
                BoxEnds { ins: vec![None; m.inputs.len()], outs: vec![None; m.outputs.len()] }
            }
        }
    }

    /// A 1 → 2^levels tree of 1×2 MMIs (splitter) or MZI switches, each
    /// output first to last top to bottom.
    fn tree(&mut self, name: &str, levels: u32, switch: bool, at: (f64, f64)) -> BoxEnds {
        if levels == 0 {
            self.add(name, "straight", vec![("length", 10.0)], at, 0, None);
            return BoxEnds { ins: vec![port(name, "o1")], outs: vec![port(name, "o2")] };
        }
        let (component, input, top, bottom, dx) =
            if switch { ("mzi2x2_2x2_phase_shifter", "o2", "o3", "o4", 400.0) } else { ("mmi1x2", "o1", "o2", "o3", 60.0) };
        // Each level's nodes, top to bottom; a node's children are 2k and 2k+1.
        let mut ends = BoxEnds::default();
        let mut level_names: Vec<Vec<String>> = Vec::new();
        for l in 0..levels {
            let count = 1usize << l;
            let spread = 30.0 * (1usize << (levels - l - 1)) as f64;
            let names: Vec<String> = (0..count)
                .map(|k| {
                    let n = format!("{name}_{l}_{k}");
                    let y = at.1 + spread * (count as f64 - 1.0 - 2.0 * k as f64);
                    let note = (l == 0 && switch).then(|| "Shine: switch tree; its heaters pick the output".to_owned());
                    self.add(&n, component, Vec::new(), (at.0 + l as f64 * dx, y), 0, note);
                    n
                })
                .collect();
            if let Some(parents) = level_names.last() {
                for (k, child) in names.iter().enumerate() {
                    let parent = &parents[k / 2];
                    let from = if k % 2 == 0 { top } else { bottom };
                    self.links.push((End::Port { inst: parent.clone(), port: from.into() }, End::Port { inst: child.clone(), port: input.into() }));
                }
            }
            level_names.push(names);
        }
        ends.ins.push(port(&level_names[0][0], input));
        for leaf in level_names.last().expect("levels > 0") {
            ends.outs.push(port(leaf, top));
            ends.outs.push(port(leaf, bottom));
        }
        ends
    }
}

/// The optical ports of the components used here.
fn optical_ports(component: &str) -> &'static [&'static str] {
    match component {
        "mmi1x2" => &["o1", "o2", "o3"],
        "straight" | "straight_heater_metal" | "ring_single" => &["o1", "o2"],
        _ => &["o1", "o2", "o3", "o4"],
    }
}

fn number(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    if s == "-0" { "0".into() } else { s }
}

fn end(e: &End) -> String {
    match e {
        End::Port { inst, port } => format!("{inst},{port}"),
        _ => unreachable!("only ports are written"),
    }
}

/// The gdsfactory YAML of document `doc`, with `spacing_um` µm between
/// neighbouring components. `source` names the project file, for the header.
pub fn export(project: &Project, doc: DocId, spacing_um: f64, source: &str) -> Result<PicExport, String> {
    let d = project.doc(doc).ok_or("no such document")?;
    let mut b = Builder { project, instances: Vec::new(), links: Vec::new(), ports: Vec::new(), notes: Vec::new(), sizes: HashMap::new(), spacing: spacing_um.max(0.0) };
    b.flatten(d.layout(), "", (0.0, 0.0), 0, true);
    if b.instances.is_empty() {
        return Err(format!(
            "“{}” has no waveguide blocks to lay out (free-space parts and electronics aren't on a chip)",
            d.name
        ));
    }
    // Every waveguide end left open is a port of the chip too: where light
    // gets in or out (a fibre coupler, a detector, another circuit).
    let used: std::collections::HashSet<(String, String)> = b
        .links
        .iter()
        .flat_map(|(f, t)| [f, t])
        .chain(b.ports.iter().map(|(_, e)| e))
        .filter_map(|e| match e {
            End::Port { inst, port } => Some((inst.clone(), port.clone())),
            _ => None,
        })
        .collect();
    let open: Vec<(String, End)> = b
        .instances
        .iter()
        .flat_map(|i| optical_ports(i.component).iter().map(move |p| (i.name.clone(), (*p).to_owned())))
        .filter(|key| !used.contains(key))
        .map(|(inst, port)| (format!("{inst}_{port}"), End::Port { inst, port }))
        .collect();
    b.ports.extend(open);
    // Port names must be unique.
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (name, _) in &mut b.ports {
        let n = seen.entry(name.clone()).or_default();
        *n += 1;
        if *n > 1 {
            *name = format!("{name}_{n}");
        }
    }

    let mut y = String::new();
    let file = format!("{}.pic.yml", super::super::workspace::slug(&d.name));
    let _ = writeln!(y, "# “{}” from Shine {}{}.", d.name, env!("CARGO_PKG_VERSION"), if source.is_empty() { String::new() } else { format!(" ({source})") });
    let _ = writeln!(y, "# A circuit for gdsfactory (https://gdsfactory.github.io), which draws the mask");
    let _ = writeln!(y, "# layout (GDS or OASIS) from it with a foundry's process kit (PDK):");
    let _ = writeln!(y, "#");
    let _ = writeln!(y, "#   import gdsfactory as gf");
    let _ = writeln!(y, "#   gf.gpdk.PDK.activate()  # or your foundry's PDK");
    let _ = writeln!(y, "#   gf.read.from_yaml(\"{file}\").write_gds(\"{}.gds\")", file.trim_end_matches(".pic.yml"));
    let _ = writeln!(y, "#");
    let _ = writeln!(y, "# Components are from gdsfactory's generic kit, in the order and arrangement of");
    let _ = writeln!(y, "# Shine's grid with {} µm between them; gdsfactory routes the waveguides.", number(spacing_um));
    let _ = writeln!(y, "# Check sizes, gaps and spacing against your PDK before tapeout.");
    if !b.notes.is_empty() {
        let _ = writeln!(y, "#\n# Not carried over:");
        for n in &b.notes {
            let _ = writeln!(y, "#   - {n}");
        }
    }
    let _ = writeln!(y, "\nname: {}", ident(&d.name));
    let _ = writeln!(y, "\ninstances:");
    for i in &b.instances {
        let _ = writeln!(y, "  {}:", i.name);
        if let Some(note) = &i.note {
            let _ = writeln!(y, "    # {note}");
        }
        let _ = writeln!(y, "    component: {}", i.component);
        if !i.settings.is_empty() {
            let _ = writeln!(y, "    settings:");
            for (k, v) in &i.settings {
                let _ = writeln!(y, "      {k}: {}", number(*v));
            }
        }
    }
    let _ = writeln!(y, "\nplacements:");
    for i in &b.instances {
        let _ = writeln!(y, "  {}:", i.name);
        let _ = writeln!(y, "    x: {}", number(i.x));
        let _ = writeln!(y, "    y: {}", number(i.y));
        if i.rotation != 0 {
            let _ = writeln!(y, "    rotation: {}", i.rotation);
        }
    }
    if !b.links.is_empty() {
        let _ = writeln!(y, "\nroutes:");
        for (k, (from, to)) in b.links.iter().enumerate() {
            let _ = writeln!(y, "  r{k}:");
            let _ = writeln!(y, "    links:");
            let _ = writeln!(y, "      {}: {}", end(from), end(to));
        }
    }
    if !b.ports.is_empty() {
        let _ = writeln!(y, "\nports:");
        for (name, e) in &b.ports {
            let _ = writeln!(y, "  {name}: {}", end(e));
        }
    }
    Ok(PicExport { yaml: y, instances: b.instances.len(), routes: b.links.len(), ports: b.ports.len(), notes: b.notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::optics::examples;
    use crate::panels::optics::model::PortRef;

    #[test]
    fn blocks_become_components_routes_and_ports() {
        let mut p = Project::new();
        let mut l = Layout::default();
        let pin = l.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "in".into() }), (0, 0));
        let split = l.add_instance(BlockRef::Builtin(Builtin::YSplitter), (3, 0));
        let ps = l.add_instance(BlockRef::Builtin(Builtin::PhaseShifter { phase: 1.0 }), (6, -1));
        let join = l.add_instance(BlockRef::Builtin(Builtin::YCombiner), (9, 0));
        let out = l.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "out".into() }), (12, 0));
        let pr = |instance, port| PortRef { instance, port };
        l.connect_optical(pr(pin, 0), pr(split, 0));
        l.connect_optical(pr(split, 0), pr(ps, 0));
        l.connect_optical(pr(ps, 0), pr(join, 0));
        l.connect_optical(pr(split, 1), pr(join, 1));
        l.connect_optical(pr(join, 0), pr(out, 0));
        let id = p.add_layout("My MZI", l, 10.0);
        let e = export(&p, id, DEFAULT_SPACING_UM, "mzi.shine").unwrap();
        // Every waveguide end is used, so the ports are just the pins.
        assert_eq!((e.instances, e.routes, e.ports), (3, 3, 2), "{}", e.yaml);
        assert!(e.yaml.contains("name: my_mzi"));
        assert!(e.yaml.contains("  y_splitter_3_0:\n    component: mmi1x2"), "{}", e.yaml);
        assert!(e.yaml.contains("component: straight_heater_metal"));
        assert!(e.yaml.contains("      y_splitter_3_0,o2: waveguide_phase_shifter_6_m1,o1"), "upper arm: {}", e.yaml);
        assert!(e.yaml.contains("      waveguide_phase_shifter_6_m1,o2: y_combiner_9_0,o3"), "into the turned combiner's upper input");
        assert!(e.yaml.contains("      y_splitter_3_0,o3: y_combiner_9_0,o2"), "lower arm into the turned combiner's lower input");
        assert!(e.yaml.contains("  in: y_splitter_3_0,o1") && e.yaml.contains("  out: y_combiner_9_0,o1"), "{}", e.yaml);
        // Columns as wide as what's in them: the pin's (40 µm), two empty
        // ones (20 each), the splitter's (26 + 40), two empty: the heater at
        // 186 µm, in the top row (y is up in gdsfactory).
        assert!(e.yaml.contains("  waveguide_phase_shifter_6_m1:\n    x: 186\n    y: 0"), "{}", e.yaml);
        assert!(e.yaml.contains("  y_splitter_3_0:\n    x: 80\n    y: -51"), "{}", e.yaml);
        assert!(e.notes.is_empty(), "{:?}", e.notes);
    }

    #[test]
    fn the_wdm_link_flattens_its_components() {
        let mut p = Project::new();
        let id = examples::wdm_link(&mut p);
        let e = export(&p, id, 100.0, "").unwrap();
        assert!(e.instances > 4 && e.routes > 4 && e.ports > 0, "{} instances, {} routes, {} ports\n{}", e.instances, e.routes, e.ports, e.yaml);
        // Every route and port names an instance that exists.
        let names: Vec<&str> = e.yaml.lines().filter_map(|l| l.strip_prefix("  ")?.strip_suffix(':')).filter(|n| !n.starts_with(' ')).collect();
        let mut in_ports = false;
        for line in e.yaml.lines().filter(|l| !l.trim_start().starts_with('#')) {
            if line.ends_with(':') && !line.starts_with(' ') {
                in_ports = line == "ports:";
            }
            if !line.contains(',') {
                continue;
            }
            // Routes join two instance ports; a chip port names one.
            let sides: Vec<&str> = line.split(": ").collect();
            for side in if in_ports { &sides[1..] } else { &sides[..] } {
                let inst = side.trim().split(',').next().unwrap();
                assert!(names.contains(&inst), "{inst} in “{line}”");
            }
        }
        // The rings are reached by beams, so their open ends are the chip's ports.
        assert!(e.yaml.contains("  add_drop_ring_4_4_o1: add_drop_ring_4_4,o1"), "{}", e.yaml);
    }

    #[test]
    fn free_space_designs_have_nothing_to_lay_out() {
        let mut p = Project::new();
        let id = p.add_layout("Bench", examples::mach_zehnder(), 10.0);
        assert!(export(&p, id, 100.0, "").unwrap_err().contains("no waveguide blocks"));
    }

    #[test]
    fn trees_are_built_from_mmis() {
        let mut p = Project::new();
        let mut l = Layout::default();
        l.add_instance(BlockRef::Builtin(Builtin::SplitterTree { levels: 2 }), (0, 0));
        let id = p.add_layout("Tree", l, 10.0);
        let e = export(&p, id, 100.0, "").unwrap();
        assert_eq!((e.instances, e.routes), (3, 2));
        assert_eq!(e.yaml.matches("component: mmi1x2").count(), 3);
    }
}
