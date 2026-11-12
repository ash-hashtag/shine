use super::error::{Error, Result};
use super::part::{Part, Port};
use super::project::{Inner, Shared, lock};
use super::records::{Direction, PortKind};
use crate::panels::optics::model::{
    BlockRef, Builtin, Export, Item, Layout, OpticComponentKind, PlacedComponent, PortRef, Terminal, cell_center,
};
use crate::panels::project::DocId;

/// A design: a circuit of parts wired together. A top-level design is what
/// you simulate; a **component** is a design others place as one part, with
/// its pins as its ports.
#[derive(Clone)]
pub struct Design {
    pub(crate) inner: Shared,
    pub(crate) id: DocId,
}

/// A part's name, as the app shows it: a pin's or value input's own name, a
/// component's name, otherwise the block's name.
fn base_name(project: &crate::panels::project::Project, layout: &Layout, item: Item) -> String {
    match item {
        Item::Box(id) => match layout.instance(id).map(|i| &i.block) {
            Some(BlockRef::Builtin(Builtin::InputPin { name } | Builtin::OutputPin { name })) => name.clone(),
            Some(BlockRef::Builtin(Builtin::Value { name, .. })) if !name.is_empty() => name.clone(),
            Some(BlockRef::Builtin(b)) => b.info().name.to_owned(),
            Some(BlockRef::Component(d)) => project.doc(*d).map_or_else(|| "Missing component".into(), |d| d.name.clone()),
            None => "Missing part".into(),
        },
        Item::Part(i) => layout.label(i).map_or_else(|| layout.components.get(i).map_or("Part", |c| c.kind.name()).to_owned(), str::to_owned),
    }
}

/// Every part of a design with its name, unique within the design: the
/// second of a kind is "Ring resonator 2", and so on.
pub(crate) fn part_names(project: &crate::panels::project::Project, doc: DocId) -> Vec<(Item, String)> {
    let Some(d) = project.doc(doc) else { return Vec::new() };
    let layout = d.layout();
    let items = layout.instances.iter().map(|i| Item::Box(i.id)).chain(
        layout
            .components
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.kind, OpticComponentKind::SignalIn | OpticComponentKind::SignalOut))
            .map(|(k, _)| Item::Part(k)),
    );
    let mut seen = std::collections::HashMap::<String, usize>::new();
    items
        .map(|item| {
            let base = base_name(project, layout, item);
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            (item, if *n == 1 { base } else { format!("{base} {n}") })
        })
        .collect()
}

impl Design {
    pub(crate) fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut lock(&self.inner))
    }

    fn layout_mut<T>(&self, f: impl FnOnce(&mut Layout) -> T) -> T {
        self.with(|inner| f(inner.project.doc_mut(self.id).expect("a design's document lives as long as it").layout_mut()))
    }

    fn part(&self, item: Item) -> Part {
        Part { inner: self.inner.clone(), doc: self.id, item }
    }

    /// The design's name.
    pub fn name(&self) -> String {
        self.with(|inner| inner.project.doc(self.id).map_or_else(String::new, |d| d.name.clone()))
    }

    /// Whether it is a component (others can place it) rather than a top-level design.
    pub fn is_component(&self) -> bool {
        self.with(|inner| inner.project.doc(self.id).is_some_and(|d| d.component.is_some()))
    }

    // ---- parts -----------------------------------------------------------------------

    /// Place a block, laid out later.
    fn add(&self, block: Builtin) -> Part {
        let item = self.with(|inner| {
            let id = inner.project.doc_mut(self.id).expect("the design").layout_mut().add_instance(BlockRef::Builtin(block), (0, 0));
            inner.unplaced.insert((self.id, Item::Box(id)));
            Item::Box(id)
        });
        self.part(item)
    }

    /// The app's block with its defaults, changed by `f`.
    fn add_default(&self, pick: impl Fn(&Builtin) -> bool, f: impl FnOnce(&mut Builtin)) -> Part {
        let mut block = Builtin::palette().into_iter().find(|b| pick(b)).expect("in the palette");
        f(&mut block);
        self.add(block)
    }

    /// A laser: one wavelength, coherent. Its light leaves from `out`.
    pub fn laser(&self, wavelength_nm: f64, power_mw: f64) -> Result<Part> {
        positive("power_mw", power_mw, true)?;
        positive("wavelength_nm", wavelength_nm, false)?;
        Ok(self.add(Builtin::Laser { amplitude: power_mw.sqrt(), phase: 0.0, wavelength_nm }))
    }

    /// A photodetector: reads the power arriving at `in`.
    pub fn detector(&self) -> Part {
        self.add(Builtin::Detector)
    }

    /// A directional coupler: `coupling` (0 to 1) of the power crosses from
    /// `in1` to `out2` (and `in2` to `out1`), a quarter turn ahead; the rest
    /// goes straight through.
    pub fn coupler(&self, coupling: f64) -> Result<Part> {
        fraction("coupling", coupling)?;
        Ok(self.add(Builtin::Coupler { coupling }))
    }

    /// A phase shifter (a heater): adds `phase` radians; a signal on its
    /// `phase` port adds that many times π more.
    pub fn phase_shifter(&self, phase: f64) -> Part {
        self.add(Builtin::PhaseShifter { phase })
    }

    /// An attenuator: `loss_db` of loss at every wavelength; a signal on its
    /// `transmission` port scales what passes (0 blocks it).
    pub fn attenuator(&self, loss_db: f64) -> Result<Part> {
        positive("loss_db", loss_db, true)?;
        Ok(self.add(Builtin::Attenuator { loss_db }))
    }

    /// A length of waveguide (the app's Delay line): silicon strip at
    /// 1550 nm unless set otherwise (`n_eff` 2.4, `n_g` 4.2, `loss` 0 dB/cm).
    pub fn waveguide(&self, length_um: f64) -> Result<Part> {
        positive("length_um", length_um, true)?;
        Ok(self.add_default(|b| matches!(b, Builtin::DelayLine { .. }), |b| {
            if let Builtin::DelayLine { length_um: l, .. } = b {
                *l = length_um;
            }
        }))
    }

    /// A Mach–Zehnder interferometer: θ sets the split (π: `in1` → `out1`,
    /// the bar state; 0: crossed), φ the phase of `in1`.
    pub fn mzi(&self, theta: f64, phi: f64) -> Part {
        self.add(Builtin::Mzi { theta, phi })
    }

    /// An all-pass ring resonator on a bus: `in` → `through`, with dips at
    /// its resonances. `n_eff` 2.4, `n_g` 4.2 and `loss` 10 dB/cm unless set.
    pub fn ring(&self, coupling: f64, length_um: f64) -> Result<Part> {
        fraction("coupling", coupling)?;
        positive("length_um", length_um, false)?;
        Ok(self.add_default(|b| matches!(b, Builtin::Ring { .. }), |b| {
            if let Builtin::Ring { coupling: c, length_um: l, .. } = b {
                *c = coupling;
                *l = length_um;
            }
        }))
    }

    /// An add–drop ring: `in` → `through` and, at its resonances, `drop`;
    /// `add` joins `drop`'s path. Lossless with `n_eff` 2.4 and `n_g` 4.2
    /// unless set.
    pub fn add_drop_ring(&self, coupling: f64, length_um: f64) -> Result<Part> {
        fraction("coupling", coupling)?;
        positive("length_um", length_um, false)?;
        Ok(self.add_default(|b| matches!(b, Builtin::AddDropRing { .. }), |b| {
            if let Builtin::AddDropRing { coupling: c, length_um: l, .. } = b {
                *c = coupling;
                *l = length_um;
            }
        }))
    }

    /// A Y-junction splitting `in` equally between `out1` and `out2`.
    pub fn y_splitter(&self) -> Part {
        self.add(Builtin::YSplitter)
    }

    /// A Y-junction joining `in1` and `in2`: only their in-phase part is guided.
    pub fn y_combiner(&self) -> Part {
        self.add(Builtin::YCombiner)
    }

    /// Two waveguides crossing: `in1` → `out2`, `in2` → `out1`.
    pub fn crossing(&self) -> Part {
        self.add(Builtin::Crossing)
    }

    /// A tree of Y-splitters sharing `in` equally between `outputs` (a power of two).
    pub fn splitter_tree(&self, outputs: u32) -> Result<Part> {
        Ok(self.add(Builtin::SplitterTree { levels: power_of_two("outputs", outputs)? }))
    }

    /// A tree of MZI switches sending `in` to output `address` (0-based) of `outputs`.
    pub fn switch_tree(&self, outputs: u32, address: u32) -> Result<Part> {
        let levels = power_of_two("outputs", outputs)?;
        if address >= outputs {
            return Err(Error::Invalid(format!("address {address} is past the last of {outputs} outputs (0 to {})", outputs - 1)));
        }
        Ok(self.add(Builtin::SwitchTree { levels, address }))
    }

    /// An optical discrete Fourier transform over `modes` waveguides.
    pub fn dft(&self, modes: u32) -> Result<Part> {
        if modes < 2 {
            return Err(Error::Invalid("a DFT needs at least 2 modes".into()));
        }
        Ok(self.add(Builtin::Dft { n: modes }))
    }

    /// An input value (usually 0 to 1) sent out on `v`: to drive a heater,
    /// a modulator or an attenuator.
    pub fn value_input(&self, value: f64) -> Part {
        self.add(Builtin::Value { name: String::new(), value })
    }

    /// A number sent out as `bits` signals, most significant first (`b7`…`b0`).
    pub fn number_input(&self, bits: u32, value: u64) -> Result<Part> {
        let bits = bit_count(bits)?;
        if value >> bits != 0 {
            return Err(Error::Invalid(format!("{value} doesn't fit in {bits} bits")));
        }
        Ok(self.add(Builtin::NumberInput { bits, value }))
    }

    /// Shows the number on its `bits` signal inputs.
    pub fn display(&self, bits: u32) -> Result<Part> {
        Ok(self.add(Builtin::Display { bits: bit_count(bits)? }))
    }

    /// Clocked memory of `bits` bits: stores `d…` on each clock tick.
    pub fn register(&self, bits: u32) -> Result<Part> {
        Ok(self.add(Builtin::Register { bits: bit_count(bits)? }))
    }

    /// Electronics written as expressions: `outputs` are expressions over
    /// its signal inputs `in1`… and the light powers `p1`… arriving at its
    /// light inputs (as in the app's function cells).
    pub fn function_cell(&self, name: &str, inputs: u32, light_inputs: u32, outputs: Vec<String>) -> Result<Part> {
        for (k, e) in outputs.iter().enumerate() {
            crate::panels::circuit::expr::parse(e).map_err(|err| Error::Invalid(format!("output {} “{e}”: {err}", k + 1)))?;
        }
        Ok(self.add(Builtin::FunctionCell {
            name: name.to_owned(),
            inputs,
            light_inputs,
            outputs,
            light_outputs: Vec::new(),
            wavelength_nm: 1550.0,
        }))
    }

    /// A copy of a component, as one part: its ports are the component's
    /// pins, its parameters the ones it exposes.
    pub fn place(&self, component: &Design) -> Result<Part> {
        if !std::sync::Arc::ptr_eq(&self.inner, &component.inner) {
            return Err(Error::Wiring("place a component from the same project (open both in one Project)".into()));
        }
        let item = self.with(|inner| {
            let catalog = inner.project.catalog();
            if inner.project.doc(component.id).is_none_or(|d| d.component.is_none()) {
                return Err(Error::Wiring(format!(
                    "“{}” is a design, not a component: make it with Project.new_component",
                    inner.project.doc(component.id).map_or("", |d| &d.name)
                )));
            }
            if !catalog.can_place(component.id, self.id) {
                return Err(Error::Wiring("a component can't contain itself".into()));
            }
            let layout = inner.project.doc_mut(self.id).expect("the design").layout_mut();
            let id = layout.add_instance(BlockRef::Component(component.id), (0, 0));
            inner.unplaced.insert((self.id, Item::Box(id)));
            Ok(Item::Box(id))
        })?;
        Ok(self.part(item))
    }

    // ---- a component's ports -------------------------------------------------------

    /// A light input of this component: light from outside enters here.
    pub fn light_input(&self, name: &str) -> Part {
        self.add(Builtin::InputPin { name: name.to_owned() })
    }

    /// A light output of this component: light leaves here.
    pub fn light_output(&self, name: &str) -> Part {
        self.add(Builtin::OutputPin { name: name.to_owned() })
    }

    fn signal_pin(&self, name: &str, kind: OpticComponentKind) -> Part {
        let item = self.with(|inner| {
            let layout = inner.project.doc_mut(self.id).expect("the design").layout_mut();
            layout.components.push(PlacedComponent { kind, pos: cell_center(0, -3) });
            let k = layout.components.len() - 1;
            layout.set_label(k, name);
            inner.unplaced.insert((self.id, Item::Part(k)));
            Item::Part(k)
        });
        self.part(item)
    }

    /// A signal input of this component: a value from outside, out on `out`
    /// inside.
    pub fn signal_input(&self, name: &str) -> Part {
        self.signal_pin(name, OpticComponentKind::SignalIn)
    }

    /// A signal output of this component: what arrives on its `in` goes out.
    pub fn signal_output(&self, name: &str) -> Part {
        self.signal_pin(name, OpticComponentKind::SignalOut)
    }

    /// Let each placed copy of this component set `part`'s parameter `param`,
    /// under the name `name`.
    pub fn expose(&self, name: &str, part: &Part, param: &str) -> Result<()> {
        self.owns(part)?;
        part.param(param)?;
        self.layout_mut(|l| {
            l.exports.retain(|e| e.name != name);
            l.exports.push(Export { name: name.to_owned(), item: part.item, param: param.to_owned() });
        });
        Ok(())
    }

    // ---- wiring ----------------------------------------------------------------------

    fn owns(&self, part: &Part) -> Result<()> {
        if part.doc == self.id && std::sync::Arc::ptr_eq(&self.inner, &part.inner) {
            Ok(())
        } else {
            Err(Error::Wiring(format!("“{}” is in another design", part.name())))
        }
    }

    /// Connect two ports: a light output to a light input with a waveguide,
    /// a signal output to a signal input with a wire. A light port takes one
    /// waveguide, so one already there is replaced.
    pub fn connect(&self, source: &Port, target: &Port) -> Result<()> {
        self.owns(&source.part)?;
        self.owns(&target.part)?;
        let (s, t) = (&source.info, &target.info);
        if s.kind != t.kind {
            return Err(Error::Wiring(format!(
                "“{}” carries {} and “{}” {}: connect light to light and signals to signals",
                source.label(),
                kind_word(s.kind),
                target.label(),
                kind_word(t.kind)
            )));
        }
        if s.direction != Direction::Out {
            return Err(Error::Wiring(format!("connections go from an output to an input: “{}” is an input", source.label())));
        }
        if t.direction != Direction::In {
            return Err(Error::Wiring(format!("connections go from an output to an input: “{}” is an output", target.label())));
        }
        match s.kind {
            PortKind::Light => {
                let (Item::Box(from), Item::Box(to)) = (source.part.item, target.part.item) else {
                    return Err(Error::Wiring("light ports are on blocks".into()));
                };
                self.layout_mut(|l| {
                    l.connect_optical(PortRef { instance: from, port: source.index }, PortRef { instance: to, port: target.index })
                });
            }
            PortKind::Signal => self.layout_mut(|l| {
                l.connect_signal(
                    Terminal { item: source.part.item, port: source.index },
                    Terminal { item: target.part.item, port: target.index },
                )
            }),
        }
        Ok(())
    }

    /// Connect parts in a row: each part's first free light output to the
    /// next part's first free light input.
    pub fn chain(&self, parts: &[Part]) -> Result<()> {
        for pair in parts.windows(2) {
            let from = pair[0].free_port(Direction::Out)?;
            let to = pair[1].free_port(Direction::In)?;
            self.connect(&from, &to)?;
        }
        Ok(())
    }

    /// Remove whatever is connected to a port.
    pub fn disconnect(&self, port: &Port) -> Result<()> {
        self.owns(&port.part)?;
        let (item, k, dir, kind) = (port.part.item, port.index, port.info.direction, port.info.kind);
        self.layout_mut(|l| match kind {
            PortKind::Light => {
                let Item::Box(id) = item else { return };
                let at = PortRef { instance: id, port: k };
                l.wires.retain(|w| if dir == Direction::Out { w.from != at } else { w.to != at });
                l.feeds.retain(|f| dir == Direction::Out || f.to != at);
            }
            PortKind::Signal => {
                let at = Terminal { item, port: k };
                l.signals.retain(|w| if dir == Direction::Out { w.from != at } else { w.to != at });
            }
        });
        Ok(())
    }

    /// Remove a part and everything connected to it.
    pub fn remove(&self, part: &Part) -> Result<()> {
        self.owns(part)?;
        let doc = self.id;
        self.with(|inner| {
            let layout = inner.project.doc_mut(doc).expect("the design").layout_mut();
            match part.item {
                Item::Box(id) => {
                    layout.remove_instances(&[id]);
                    layout.exports.retain(|e| e.item != part.item);
                    inner.unplaced.remove(&(doc, part.item));
                    Ok(())
                }
                Item::Part(_) => Err(Error::Unsupported("signal pins can't be removed yet; make the component again".into())),
            }
        })
    }

    /// Every part, in the order added.
    pub fn parts(&self) -> Vec<Part> {
        let items = self.with(|inner| part_names(&inner.project, self.id));
        items.into_iter().map(|(item, _)| self.part(item)).collect()
    }

    /// A part by name ("Ring resonator", "Directional coupler 2", a pin's name…).
    pub fn find_part(&self, name: &str) -> Result<Part> {
        let names = self.with(|inner| part_names(&inner.project, self.id));
        match names.iter().find(|(_, n)| n == name) {
            Some((item, _)) => Ok(self.part(*item)),
            None => Err(Error::NoSuchPart { design: self.name(), name: name.to_owned(), available: names.into_iter().map(|(_, n)| n).collect() }),
        }
    }

    /// The design as text: its parts, their ports and how they connect.
    pub fn describe(&self) -> String {
        use std::fmt::Write as _;
        let parts = self.parts();
        let mut s = String::new();
        let _ = writeln!(s, "{} “{}”: {} parts", if self.is_component() { "Component" } else { "Design" }, self.name(), parts.len());
        for p in &parts {
            let ports: Vec<String> = p.ports().iter().map(|i| format!("{}{}", if i.direction == Direction::In { "←" } else { "→" }, i.name)).collect();
            let _ = writeln!(s, "  {} ({}) {}", p.name(), p.kind(), ports.join(" "));
        }
        let wires = self.with(|inner| {
            let names = part_names(&inner.project, self.id);
            let name = |item: Item| names.iter().find(|(i, _)| *i == item).map_or("?".to_owned(), |(_, n)| n.clone());
            let catalog = inner.project.catalog();
            let layout = inner.project.doc(self.id).expect("the design").layout();
            let port_name = |id, k: usize, out: bool| {
                layout.instance(id).map_or_else(String::new, |i| {
                    let d = catalog.display(&i.block);
                    (if out { d.outputs } else { d.inputs }).get(k).cloned().unwrap_or_default()
                })
            };
            layout
                .wires
                .iter()
                .map(|w| {
                    format!(
                        "  {}.{} → {}.{}",
                        name(Item::Box(w.from.instance)),
                        port_name(w.from.instance, w.from.port, true),
                        name(Item::Box(w.to.instance)),
                        port_name(w.to.instance, w.to.port, false)
                    )
                })
                .collect::<Vec<_>>()
        });
        if !wires.is_empty() {
            let _ = writeln!(s, "Waveguides:");
            for w in wires {
                let _ = writeln!(s, "{w}");
            }
        }
        s
    }
}

fn kind_word(k: PortKind) -> &'static str {
    match k {
        PortKind::Light => "light",
        PortKind::Signal => "a signal",
    }
}

fn fraction(name: &str, v: f64) -> Result<()> {
    if (0.0..=1.0).contains(&v) { Ok(()) } else { Err(Error::Invalid(format!("{name} is a fraction from 0 to 1, not {v}"))) }
}

fn positive(name: &str, v: f64, zero: bool) -> Result<()> {
    if v.is_finite() && (v > 0.0 || (zero && v == 0.0)) {
        Ok(())
    } else {
        Err(Error::Invalid(format!("{name} must be {}, not {v}", if zero { "0 or more" } else { "more than 0" })))
    }
}

fn power_of_two(name: &str, n: u32) -> Result<u32> {
    if n >= 1 && n.is_power_of_two() && n <= 64 {
        Ok(n.trailing_zeros())
    } else {
        Err(Error::Invalid(format!("{name} must be a power of two from 1 to 64, not {n}")))
    }
}

fn bit_count(bits: u32) -> Result<u32> {
    if (1..=16).contains(&bits) { Ok(bits) } else { Err(Error::Invalid(format!("bits must be 1 to 16, not {bits}"))) }
}

impl std::fmt::Debug for Design {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", if self.is_component() { "Component" } else { "Design" }, self.name())
    }
}

impl PartialEq for Design {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.inner, &other.inner) && self.id == other.id
    }
}
