//! A project: layouts, some of which are published as reusable components.
//!
//! A component is a layout with a name, initials and a description. Its
//! ports come from its parts: Input/Output ports for light and Signal
//! in/out pins for electrical signals, ordered top to bottom. Instances
//! refer to the layout by id, so editing it updates every instance.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::circuit::sim::{self, Documents, Machine, Options, Outcome};
use super::file_format;
use super::optics::OpticsPanel;
use super::optics::model::{BlockRef, Builtin, Item, Layout, PortCounts};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DocId(pub u64);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComponentMeta {
    pub initials: String,
    pub description: String,
}

/// What a document holds. Layouts are the only kind today; the enum leaves
/// room for others.
pub enum Content {
    Layout(Box<OpticsPanel>),
}

/// Port names of a document used as a component.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interface {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub signal_inputs: Vec<String>,
    pub signal_outputs: Vec<String>,
}

impl Interface {
    pub fn counts(&self) -> PortCounts {
        PortCounts {
            optical_in: self.inputs.len(),
            optical_out: self.outputs.len(),
            signal_in: self.signal_inputs.len(),
            signal_out: self.signal_outputs.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.counts() == PortCounts::default()
    }
}

/// Port names of a layout, from its port and pin parts.
pub fn layout_interface(layout: &Layout) -> Interface {
    let iface = sim::interface(layout);
    let name = |item: &Item, default: String| match item {
        Item::Part(i) => layout.label(*i).map(str::to_owned).unwrap_or(default),
        Item::Box(id) => match layout.instance(*id).map(|i| &i.block) {
            Some(BlockRef::Builtin(Builtin::InputPin { name } | Builtin::OutputPin { name })) => name.clone(),
            _ => default,
        },
    };
    let names = |items: &[Item], prefix: &str| -> Vec<String> {
        items.iter().enumerate().map(|(k, item)| name(item, format!("{prefix}{}", k + 1))).collect()
    };
    let parts = |indices: &[usize]| indices.iter().map(|&i| Item::Part(i)).collect::<Vec<_>>();
    Interface {
        inputs: names(&iface.optical_in, "in"),
        outputs: names(&iface.optical_out, "out"),
        signal_inputs: names(&parts(&iface.signal_in), "s"),
        signal_outputs: names(&parts(&iface.signal_out), "q"),
    }
}

pub struct Document {
    pub id: DocId,
    pub name: String,
    /// Set when the document is published as a component.
    pub component: Option<ComponentMeta>,
    pub content: Content,
}

impl Document {
    pub fn panel(&self) -> &OpticsPanel {
        match &self.content {
            Content::Layout(p) => p,
        }
    }

    pub fn panel_mut(&mut self) -> &mut OpticsPanel {
        match &mut self.content {
            Content::Layout(p) => p,
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.panel().layout
    }

    pub fn interface(&self) -> Interface {
        layout_interface(self.layout())
    }

    /// Documents this one uses directly.
    pub fn references(&self) -> Vec<DocId> {
        let mut refs: Vec<DocId> = self
            .layout()
            .instances
            .iter()
            .filter_map(|i| match i.block {
                BlockRef::Component(id) => Some(id),
                BlockRef::Builtin(_) => None,
            })
            .collect();
        refs.sort();
        refs.dedup();
        refs
    }

    pub fn initials(&self) -> String {
        self.component
            .as_ref()
            .map(|m| m.initials.clone())
            .filter(|i| !i.is_empty())
            .unwrap_or_else(|| super::ui_kit::initials(&self.name))
    }

    fn to_file(&self) -> DocumentFile {
        DocumentFile {
            id: self.id,
            name: self.name.clone(),
            component: self.component.clone(),
            cell_um: self.panel().cell_um,
            layout: self.layout().clone(),
        }
    }

    fn from_file(file: DocumentFile) -> Self {
        Self {
            id: file.id,
            name: file.name,
            component: file.component,
            content: Content::Layout(Box::new(OpticsPanel::with_layout(file.layout, file.cell_um))),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    next_id: u64,
    documents: Vec<DocumentFile>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DocumentFile {
    id: DocId,
    name: String,
    component: Option<ComponentMeta>,
    cell_um: f64,
    layout: Layout,
}

pub(crate) mod v4;

/// Format 3: layouts had no laser feeds (waveguides from a laser into a box).
pub(crate) mod v3 {
    use serde::{Deserialize, Serialize};

    use super::v4::{Instance, Layout4};
    use super::{ComponentMeta, DocId, DocumentFile};
    use crate::panels::optics::model::{Line, PlacedComponent, SignalWire, Wire};

    #[derive(Serialize, Deserialize)]
    pub struct Layout3 {
        pub components: Vec<PlacedComponent>,
        pub lines: Vec<Line>,
        pub instances: Vec<Instance>,
        pub wires: Vec<Wire>,
        pub signals: Vec<SignalWire>,
        pub next_instance: u32,
        pub labels: Vec<(usize, String)>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Document {
        pub id: DocId,
        pub name: String,
        pub component: Option<ComponentMeta>,
        pub cell_um: f64,
        pub layout: Layout3,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Project {
        pub next_id: u64,
        pub documents: Vec<Document>,
    }

    pub fn upgrade(d: Document) -> DocumentFile {
        let l = d.layout;
        DocumentFile {
            id: d.id,
            name: d.name,
            component: d.component,
            cell_um: d.cell_um,
            layout: Layout4 {
                components: l.components,
                lines: l.lines,
                instances: l.instances,
                wires: l.wires,
                signals: l.signals,
                next_instance: l.next_instance,
                labels: l.labels,
                feeds: Vec::new(),
            }
            .upgrade(),
        }
    }
}

/// Format 2: lasers had no source or colour, and every laser was coherent
/// with every other. They become lasers on one shared source.
pub(crate) mod v2 {
    use egui::{Pos2, Vec2};
    use serde::{Deserialize, Serialize};

    use super::v4::{Instance, Layout4};
    use super::{ComponentMeta, DocId, DocumentFile};
    use crate::panels::optics::model::{Line, OpticComponentKind, PlacedComponent, SignalWire, Wire};

    #[derive(Clone, Copy, Serialize, Deserialize)]
    pub enum Kind {
        Laser { amplitude: f32, phase: f32, wavelength: f32, dir: Vec2 },
        BeamSplitter { reflectivity: f32, transparency: f32, normal: Vec2 },
        Mirror { normal: Vec2 },
        Lens,
        PhaseShifter { phase: f32 },
        Attenuator { transmission: f32 },
        Detector { threshold: f32 },
        InputPort { dir: Vec2 },
        OutputPort,
        SignalIn,
        SignalOut,
    }

    #[derive(Clone, Copy, Serialize, Deserialize)]
    pub struct Part {
        pub kind: Kind,
        pub pos: Pos2,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Layout2 {
        pub components: Vec<Part>,
        pub lines: Vec<Line>,
        pub instances: Vec<Instance>,
        pub wires: Vec<Wire>,
        pub signals: Vec<SignalWire>,
        pub next_instance: u32,
        pub labels: Vec<(usize, String)>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Document {
        pub id: DocId,
        pub name: String,
        pub component: Option<ComponentMeta>,
        pub cell_um: f64,
        pub layout: Layout2,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Project {
        pub next_id: u64,
        pub documents: Vec<Document>,
    }

    pub fn kind(k: Kind) -> OpticComponentKind {
        match k {
            Kind::Laser { amplitude, phase, wavelength, dir } => {
                OpticComponentKind::Laser { amplitude, phase, wavelength, dir, source: 1, color: None }
            }
            Kind::BeamSplitter { reflectivity, transparency, normal } => {
                OpticComponentKind::BeamSplitter { reflectivity, transparency, normal }
            }
            Kind::Mirror { normal } => OpticComponentKind::Mirror { normal },
            Kind::Lens => OpticComponentKind::Lens,
            Kind::PhaseShifter { phase } => OpticComponentKind::PhaseShifter { phase },
            Kind::Attenuator { transmission } => OpticComponentKind::Attenuator { transmission },
            Kind::Detector { threshold } => OpticComponentKind::Detector { threshold },
            Kind::InputPort { dir } => OpticComponentKind::InputPort { dir },
            Kind::OutputPort => OpticComponentKind::OutputPort,
            Kind::SignalIn => OpticComponentKind::SignalIn,
            Kind::SignalOut => OpticComponentKind::SignalOut,
        }
    }

    pub fn upgrade(d: Document) -> DocumentFile {
        let l = d.layout;
        DocumentFile {
            id: d.id,
            name: d.name,
            component: d.component,
            cell_um: d.cell_um,
            layout: Layout4 {
                components: l.components.into_iter().map(|p| PlacedComponent { kind: kind(p.kind), pos: p.pos }).collect(),
                lines: l.lines,
                instances: l.instances,
                wires: l.wires,
                signals: l.signals,
                next_instance: l.next_instance,
                labels: l.labels,
                feeds: Vec::new(),
            }
            .upgrade(),
        }
    }
}

/// Format 1: layouts held only primitives (detectors had no threshold) and
/// schematics were a separate document kind. Both become layouts.
pub(crate) mod v1 {
    use egui::{Pos2, Vec2};
    use serde::{Deserialize, Serialize};

    use super::{ComponentMeta, DocId, DocumentFile};
    use crate::panels::schematic::model::Schematic;
    use crate::panels::optics::model::{DETECTOR, GRID, Instance, Layout, Line, OpticComponentKind, PlacedComponent};

    #[derive(Clone, Copy, Serialize, Deserialize)]
    pub enum Kind {
        Laser { amplitude: f32, phase: f32, wavelength: f32, dir: Vec2 },
        BeamSplitter { reflectivity: f32, transparency: f32, normal: Vec2 },
        Mirror { normal: Vec2 },
        Lens,
        PhaseShifter { phase: f32 },
        Attenuator { transmission: f32 },
        Detector,
        InputPort { dir: Vec2 },
        OutputPort,
    }

    #[derive(Clone, Copy, Serialize, Deserialize)]
    pub struct Part {
        pub kind: Kind,
        pub pos: Pos2,
    }

    #[derive(Serialize, Deserialize)]
    pub enum Content {
        Layout { cell_um: f64, components: Vec<Part>, lines: Vec<Line> },
        Schematic { schematic: super::v4::Schematic },
    }

    #[derive(Serialize, Deserialize)]
    pub struct Document {
        pub id: DocId,
        pub name: String,
        pub component: Option<ComponentMeta>,
        pub content: Content,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Project {
        pub next_id: u64,
        pub documents: Vec<Document>,
    }

    fn part(p: Part) -> PlacedComponent {
        let kind = match p.kind {
            // Lasers were all mutually coherent then: keep them on one source.
            Kind::Laser { amplitude, phase, wavelength, dir } => {
                OpticComponentKind::Laser { amplitude, phase, wavelength, dir, source: 1, color: None }
            }
            Kind::BeamSplitter { reflectivity, transparency, normal } => {
                OpticComponentKind::BeamSplitter { reflectivity, transparency, normal }
            }
            Kind::Mirror { normal } => OpticComponentKind::Mirror { normal },
            Kind::Lens => OpticComponentKind::Lens,
            Kind::PhaseShifter { phase } => OpticComponentKind::PhaseShifter { phase },
            Kind::Attenuator { transmission } => OpticComponentKind::Attenuator { transmission },
            Kind::Detector => DETECTOR,
            Kind::InputPort { dir } => OpticComponentKind::InputPort { dir },
            Kind::OutputPort => OpticComponentKind::OutputPort,
        };
        PlacedComponent { kind, pos: p.pos }
    }

    /// A schematic becomes a layout of boxes on the grid, keeping its waveguides.
    pub fn schematic_to_layout(s: Schematic) -> Layout {
        let instances: Vec<Instance> = s
            .instances
            .iter()
            .map(|inst| Instance {
                id: inst.id,
                block: inst.block.clone(),
                cell: ((inst.pos.x / GRID).round() as i32, (inst.pos.y / GRID).round() as i32),
            })
            .collect();
        Layout {
            next_instance: instances.iter().map(|i| i.id.0 + 1).max().unwrap_or(0),
            instances,
            wires: s.wires,
            ..Layout::default()
        }
    }

    pub fn upgrade(d: Document) -> DocumentFile {
        let (cell_um, layout) = match d.content {
            Content::Layout { cell_um, components, lines } => (
                cell_um,
                Layout {
                    components: components.into_iter().map(part).collect(),
                    lines,
                    ..Layout::default()
                },
            ),
            Content::Schematic { schematic } => (10.0, schematic_to_layout(schematic.upgrade())),
        };
        DocumentFile {
            id: d.id,
            name: d.name,
            component: d.component,
            cell_um,
            layout,
        }
    }
}

/// Read-only summary of every document, for drawing and placing instances.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogEntry {
    pub id: DocId,
    pub name: String,
    pub initials: String,
    pub description: String,
    pub interface: Interface,
    pub is_component: bool,
    /// Every document this one uses, directly or not.
    pub depends_on: BTreeSet<DocId>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub entries: Vec<CatalogEntry>,
}

/// How an instance looks and connects.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockDisplay {
    pub name: String,
    pub initials: String,
    pub description: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub signal_inputs: Vec<String>,
    pub signal_outputs: Vec<String>,
    pub component: Option<DocId>,
}

impl BlockDisplay {
    pub fn counts(&self) -> PortCounts {
        PortCounts {
            optical_in: self.inputs.len(),
            optical_out: self.outputs.len(),
            signal_in: self.signal_inputs.len(),
            signal_out: self.signal_outputs.len(),
        }
    }
}

impl Catalog {
    pub fn get(&self, id: DocId) -> Option<&CatalogEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn components(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.entries.iter().filter(|e| e.is_component)
    }

    /// Whether `component` may be placed inside document `into` without
    /// making a document contain itself.
    pub fn can_place(&self, component: DocId, into: DocId) -> bool {
        component != into && self.get(component).is_some_and(|e| !e.depends_on.contains(&into))
    }

    pub fn counts(&self, block: &BlockRef) -> PortCounts {
        self.display(block).counts()
    }

    pub fn display(&self, block: &BlockRef) -> BlockDisplay {
        match block {
            BlockRef::Builtin(b) => {
                let info = b.info();
                let name = match b {
                    Builtin::InputPin { name } | Builtin::OutputPin { name } => format!("{} “{name}”", info.name),
                    Builtin::FunctionCell { name, .. } if !name.is_empty() => name.clone(),
                    _ => info.name.to_owned(),
                };
                BlockDisplay {
                    name,
                    initials: info.initials.to_owned(),
                    description: info.description.to_owned(),
                    inputs: b.inputs(),
                    outputs: b.outputs(),
                    signal_inputs: b.signal_inputs(),
                    signal_outputs: b.signal_outputs(),
                    component: None,
                }
            }
            BlockRef::Component(id) => match self.get(*id) {
                Some(e) => BlockDisplay {
                    name: e.name.clone(),
                    initials: e.initials.clone(),
                    description: e.description.clone(),
                    inputs: e.interface.inputs.clone(),
                    outputs: e.interface.outputs.clone(),
                    signal_inputs: e.interface.signal_inputs.clone(),
                    signal_outputs: e.interface.signal_outputs.clone(),
                    component: Some(*id),
                },
                None => BlockDisplay {
                    name: "Missing component".into(),
                    initials: "?".into(),
                    description: "This component was deleted.".into(),
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    signal_inputs: Vec::new(),
                    signal_outputs: Vec::new(),
                    component: Some(*id),
                },
            },
        }
    }
}

pub struct Project {
    pub documents: Vec<Document>,
    next_id: u64,
    /// Where the project was last saved or opened from.
    pub path: Option<PathBuf>,
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Documents for Project {
    fn layout(&self, id: DocId) -> Option<(&Layout, f64)> {
        self.doc(id).map(|d| (d.layout(), d.panel().cell_um))
    }

    fn name(&self, id: DocId) -> String {
        self.doc(id).map_or_else(|| "a deleted component".into(), |d| d.name.clone())
    }
}

impl Project {
    pub fn new() -> Self {
        Self {
            documents: Vec::new(),
            next_id: 1,
            path: None,
        }
    }

    fn allocate(&mut self) -> DocId {
        let id = DocId(self.next_id);
        self.next_id += 1;
        id
    }

    /// `base`, or `base 2`, `base 3`... whichever is free.
    pub fn unique_name(&self, base: &str) -> String {
        let taken = |n: &str| self.documents.iter().any(|d| d.name == n);
        if !taken(base) {
            return base.to_owned();
        }
        (2..).map(|k| format!("{base} {k}")).find(|n| !taken(n)).expect("unbounded")
    }

    pub fn add_layout(&mut self, name: &str, layout: Layout, cell_um: f64) -> DocId {
        let id = self.allocate();
        let name = self.unique_name(name);
        self.documents.push(Document {
            id,
            name,
            component: None,
            content: Content::Layout(Box::new(OpticsPanel::with_layout(layout, cell_um))),
        });
        id
    }

    /// Add a layout already published as a component.
    pub fn add_component(&mut self, name: &str, initials: &str, description: &str, layout: Layout, cell_um: f64) -> DocId {
        let id = self.add_layout(name, layout, cell_um);
        self.doc_mut(id).expect("just added").component = Some(ComponentMeta {
            initials: initials.into(),
            description: description.into(),
        });
        id
    }

    pub fn doc(&self, id: DocId) -> Option<&Document> {
        self.documents.iter().find(|d| d.id == id)
    }

    pub fn doc_mut(&mut self, id: DocId) -> Option<&mut Document> {
        self.documents.iter_mut().find(|d| d.id == id)
    }

    pub fn find(&self, name: &str) -> Option<DocId> {
        self.documents.iter().find(|d| d.name == name).map(|d| d.id)
    }

    /// Documents that use `id` directly.
    pub fn users_of(&self, id: DocId) -> Vec<DocId> {
        self.documents.iter().filter(|d| d.references().contains(&id)).map(|d| d.id).collect()
    }

    /// Delete a document unless another document uses it.
    pub fn remove(&mut self, id: DocId) -> Result<(), String> {
        if let Some(user) = self.users_of(id).first().and_then(|&u| self.doc(u)) {
            return Err(format!("It is used in “{}”", user.name));
        }
        self.documents.retain(|d| d.id != id);
        Ok(())
    }

    fn dependencies(&self, id: DocId) -> BTreeSet<DocId> {
        let mut seen = BTreeSet::new();
        let mut stack = self.doc(id).map(|d| d.references()).unwrap_or_default();
        while let Some(next) = stack.pop() {
            if seen.insert(next)
                && let Some(d) = self.doc(next)
            {
                stack.extend(d.references());
            }
        }
        seen
    }

    pub fn catalog(&self) -> Catalog {
        Catalog {
            entries: self
                .documents
                .iter()
                .map(|d| CatalogEntry {
                    id: d.id,
                    name: d.name.clone(),
                    initials: d.initials(),
                    description: d.component.as_ref().map(|m| m.description.clone()).unwrap_or_default(),
                    interface: d.interface(),
                    is_component: d.component.is_some(),
                    depends_on: self.dependencies(d.id),
                })
                .collect(),
        }
    }

    /// Settle a document's optics and electronics with the given register state.
    pub fn simulate(&self, id: DocId, machine: &Machine) -> Outcome {
        match self.doc(id) {
            Some(d) => sim::simulate(d.layout(), d.panel().cell_um, self, machine, Some(id), &Options::default()),
            None => Outcome::failed("no such document"),
        }
    }

    /// Changes whenever any document's content or component info changes.
    pub fn fingerprint(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for d in &self.documents {
            postcard::to_allocvec(&d.to_file()).expect("serialisable").hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Encode the project in Shine's binary file format.
    pub fn to_bytes(&self) -> Vec<u8> {
        file_format::encode(&ProjectFile {
            next_id: self.next_id,
            documents: self.documents.iter().map(Document::to_file).collect(),
        })
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let file: ProjectFile = match file_format::version(bytes)? {
            1 => {
                let old: v1::Project = file_format::decode_body(bytes)?;
                ProjectFile {
                    next_id: old.next_id,
                    documents: old.documents.into_iter().map(v1::upgrade).collect(),
                }
            }
            2 => {
                let old: v2::Project = file_format::decode_body(bytes)?;
                ProjectFile {
                    next_id: old.next_id,
                    documents: old.documents.into_iter().map(v2::upgrade).collect(),
                }
            }
            3 => {
                let old: v3::Project = file_format::decode_body(bytes)?;
                ProjectFile {
                    next_id: old.next_id,
                    documents: old.documents.into_iter().map(v3::upgrade).collect(),
                }
            }
            4 => {
                let old: v4::Project = file_format::decode_body(bytes)?;
                ProjectFile {
                    next_id: old.next_id,
                    documents: old.documents.into_iter().map(v4::upgrade).collect(),
                }
            }
            _ => file_format::decode(bytes)?,
        };
        let documents: Vec<Document> = file.documents.into_iter().map(Document::from_file).collect();
        let next_id = documents.iter().map(|d| d.id.0 + 1).max().unwrap_or(1).max(file.next_id);
        Ok(Self {
            documents,
            next_id,
            path: None,
        })
    }

    /// Whether `bytes` are a Shine project file.
    pub fn is_project(bytes: &[u8]) -> bool {
        file_format::is_shine(bytes)
    }
}

#[cfg(test)]
mod tests;
