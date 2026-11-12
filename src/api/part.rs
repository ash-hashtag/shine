use super::design::part_names;
use super::error::{Error, Result};
use super::project::{Inner, Shared, lock};
use super::records::{Direction, Param, PortInfo, PortKind};
use crate::panels::circuit::sweep;
use crate::panels::optics::model::{BlockRef, Item, Layout, PortRef, cell_center};
use crate::panels::project::DocId;

/// One block placed in a design: built in (a coupler, a ring…) or a copy
/// of a component.
#[derive(Clone)]
pub struct Part {
    pub(crate) inner: Shared,
    pub(crate) doc: DocId,
    pub(crate) item: Item,
}

/// One of a part's ports: where a waveguide or a signal wire connects.
#[derive(Clone)]
pub struct Port {
    pub(crate) part: Part,
    pub(crate) info: PortInfo,
    /// Its position among the part's ports of its kind and direction.
    pub(crate) index: usize,
}

impl Port {
    pub fn part(&self) -> Part {
        self.part.clone()
    }
    pub fn name(&self) -> String {
        self.info.name.clone()
    }
    pub fn kind(&self) -> PortKind {
        self.info.kind
    }
    pub fn direction(&self) -> Direction {
        self.info.direction
    }
    /// "Ring resonator.through".
    pub fn label(&self) -> String {
        format!("{}.{}", self.part.name(), self.info.name)
    }
}

/// A part's ports, in order: light in, light out, signal in, signal out.
pub(crate) fn ports_of(inner: &Inner, doc: DocId, item: Item) -> Vec<(PortInfo, usize)> {
    let Some(d) = inner.project.doc(doc) else { return Vec::new() };
    let layout = d.layout();
    let (light_in, light_out, signal_in, signal_out): (Vec<String>, Vec<String>, Vec<String>, Vec<String>) = match item {
        Item::Box(id) => match layout.instance(id) {
            Some(i) => {
                let b = inner.project.catalog().display(&i.block);
                // A component's pins face into it: an input pin's light
                // leaves on `out`, an output pin's arrives on `in`.
                match &i.block {
                    BlockRef::Builtin(crate::panels::optics::model::Builtin::InputPin { .. }) => (Vec::new(), b.outputs, Vec::new(), Vec::new()),
                    BlockRef::Builtin(crate::panels::optics::model::Builtin::OutputPin { .. }) => (b.inputs, Vec::new(), Vec::new(), Vec::new()),
                    _ => (b.inputs, b.outputs, b.signal_inputs, b.signal_outputs),
                }
            }
            None => Default::default(),
        },
        Item::Part(k) => match layout.components.get(k) {
            Some(c) => (
                Vec::new(),
                Vec::new(),
                c.kind.signal_inputs().iter().map(|s| s.to_string()).collect(),
                c.kind.signal_outputs().iter().map(|s| s.to_string()).collect(),
            ),
            None => Default::default(),
        },
    };
    let mut out = Vec::new();
    for (names, kind, direction) in [
        (light_in, PortKind::Light, Direction::In),
        (light_out, PortKind::Light, Direction::Out),
        (signal_in, PortKind::Signal, Direction::In),
        (signal_out, PortKind::Signal, Direction::Out),
    ] {
        out.extend(names.into_iter().enumerate().map(|(k, name)| (PortInfo { name, kind, direction }, k)));
    }
    out
}

impl Part {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut lock(&self.inner))
    }

    fn layout<T>(&self, f: impl FnOnce(&Layout) -> T) -> T {
        self.with(|inner| f(inner.project.doc(self.doc).expect("the part's design").layout()))
    }

    /// Its name in its design: the block's name, numbered from the second
    /// of a kind ("Ring resonator 2"), or a pin's or component's name.
    pub fn name(&self) -> String {
        self.with(|inner| part_names(&inner.project, self.doc).into_iter().find(|(i, _)| *i == self.item).map(|(_, n)| n).unwrap_or_default())
    }

    /// What it is: the block's name ("Ring resonator") or the component's.
    pub fn kind(&self) -> String {
        self.with(|inner| {
            let layout = inner.project.doc(self.doc).expect("the part's design").layout();
            match self.item {
                Item::Box(id) => layout.instance(id).map_or_else(String::new, |i| match &i.block {
                    BlockRef::Builtin(b) => b.info().name.to_owned(),
                    BlockRef::Component(d) => inner.project.doc(*d).map_or_else(String::new, |d| d.name.clone()),
                }),
                Item::Part(k) => layout.components.get(k).map_or("", |c| c.kind.name()).to_owned(),
            }
        })
    }

    /// Its ports: name, light or signal, in or out.
    pub fn ports(&self) -> Vec<PortInfo> {
        self.with(|inner| ports_of(inner, self.doc, self.item).into_iter().map(|p| p.0).collect())
    }

    /// A port by name (`in1`, `through`, `phase`…).
    pub fn port(&self, name: &str) -> Result<Port> {
        let ports = self.with(|inner| ports_of(inner, self.doc, self.item));
        let found: Vec<&(PortInfo, usize)> = ports.iter().filter(|(p, _)| p.name == name).collect();
        match found.as_slice() {
            [(info, index)] => Ok(Port { part: self.clone(), info: info.clone(), index: *index }),
            [] => Err(Error::NoSuchPort { part: self.name(), port: name.to_owned(), available: ports.iter().map(|p| p.0.name.clone()).collect() }),
            _ => Err(Error::Wiring(format!("“{}” has more than one port called “{name}”", self.name()))),
        }
    }

    /// The first light port in `direction` with nothing connected.
    pub(crate) fn free_port(&self, direction: Direction) -> Result<Port> {
        let ports = self.with(|inner| ports_of(inner, self.doc, self.item));
        let busy = |index: usize| -> bool {
            self.layout(|l| {
                let Item::Box(id) = self.item else { return true };
                let at = PortRef { instance: id, port: index };
                match direction {
                    Direction::Out => l.wires.iter().any(|w| w.from == at),
                    Direction::In => l.wires.iter().any(|w| w.to == at) || l.feeds.iter().any(|f| f.to == at),
                }
            })
        };
        ports
            .into_iter()
            .find(|(p, k)| p.kind == PortKind::Light && p.direction == direction && !busy(*k))
            .map(|(info, index)| Port { part: self.clone(), info, index })
            .ok_or_else(|| {
                Error::Wiring(format!(
                    "“{}” has no free light {}",
                    self.name(),
                    if direction == Direction::Out { "output" } else { "input" }
                ))
            })
    }

    /// Its parameters: name, unit, value and typical range. A component's
    /// are the ones it exposes.
    pub fn params(&self) -> Vec<Param> {
        self.with(|inner| {
            let layout = inner.project.doc(self.doc).expect("the part's design").layout();
            sweep::params_in(layout, self.item, &inner.project)
                .into_iter()
                .map(|p| {
                    let value = sweep::get_in(layout, self.item, &p.name, &inner.project).unwrap_or(f64::NAN);
                    Param { value, unit: p.unit.to_owned(), min: p.min, max: p.max, name: p.name }
                })
                .collect()
        })
    }

    pub(crate) fn param(&self, name: &str) -> Result<Param> {
        let params = self.params();
        params.iter().find(|p| p.name == name).cloned().ok_or_else(|| Error::NoSuchParameter {
            part: self.name(),
            param: name.to_owned(),
            available: params.iter().map(|p| p.name.clone()).collect(),
        })
    }

    /// A parameter's value.
    pub fn get(&self, param: &str) -> Result<f64> {
        Ok(self.param(param)?.value)
    }

    /// Set a parameter; returns the part, so settings chain:
    /// `ring.set("n_g", 4.2)?.set("loss", 3.0)?`.
    pub fn set(&self, param: &str, value: f64) -> Result<Part> {
        self.param(param)?;
        if !value.is_finite() {
            return Err(Error::Invalid(format!("{param} must be a number, not {value}")));
        }
        self.with(|inner| sweep::set(inner.project.doc_mut(self.doc).expect("the part's design").layout_mut(), self.item, param, value));
        Ok(self.clone())
    }

    /// Fix its position in the app: its top-left cell, columns to the right
    /// and rows down. Parts left unplaced are laid out when the project is
    /// saved.
    pub fn place_at(&self, column: i32, row: i32) -> Part {
        self.with(|inner| {
            inner.unplaced.remove(&(self.doc, self.item));
            let layout = inner.project.doc_mut(self.doc).expect("the part's design").layout_mut();
            match self.item {
                Item::Box(id) => {
                    if let Some(i) = layout.instances.iter_mut().find(|i| i.id == id) {
                        i.cell = (column, row);
                    }
                }
                Item::Part(k) => {
                    if let Some(c) = layout.components.get_mut(k) {
                        c.pos = cell_center(column, row);
                    }
                }
            }
        });
        self.clone()
    }
}

impl PartialEq for Part {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.inner, &other.inner) && self.doc == other.doc && self.item == other.item
    }
}

impl std::fmt::Debug for Part {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Part({})", self.name())
    }
}

impl std::fmt::Debug for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Port({})", self.label())
    }
}
