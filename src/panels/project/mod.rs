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
use super::appearance::Appearance;
use super::file_format;
use super::optics::model::{BlockRef, Builtin, Item, Layout, PortCounts};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DocId(pub u64);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComponentMeta {
    pub initials: String,
    pub description: String,
    /// Colour, outline and picture of its boxes.
    pub appearance: Appearance,
}

/// What a document holds. Layouts are the only kind today; the enum leaves
/// room for others.
pub enum Content {
    Layout(Box<DocumentState>),
}

/// A document's design and what goes with it: in the app, its editor (which
/// keeps the layout, scale and registers with the view and selection).
#[cfg(feature = "gui")]
pub type DocumentState = super::optics::OpticsPanel;

/// A document's design, scale and registers: what a document is without the
/// editor (the library).
#[cfg(not(feature = "gui"))]
#[derive(Clone, Debug, Default)]
pub struct DocumentState {
    pub layout: Layout,
    /// Physical size of one grid cell in micrometres.
    pub cell_um: f64,
    /// Register contents and clock count.
    pub machine: Machine,
}

#[cfg(not(feature = "gui"))]
impl DocumentState {
    pub fn with_layout(layout: Layout, cell_um: f64) -> Self {
        Self { layout, cell_um, machine: Machine::default() }
    }
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
    /// Counts changes, so the fingerprint is only computed again after one.
    /// [`Self::layout_mut`] and [`Self::touch`] count one; the editor
    /// touches the open document when it changes. Not saved.
    revision: u64,
    /// The fingerprint, and the revision it was computed at.
    fingerprint: std::sync::Mutex<Option<(u64, u64)>>,
}

impl Document {
    fn new(id: DocId, name: String, component: Option<ComponentMeta>, content: Content) -> Self {
        Self { id, name, component, content, revision: 0, fingerprint: std::sync::Mutex::new(None) }
    }

    /// Count a change to the document's name, component information or
    /// content made other than through [`Self::layout_mut`].
    pub fn touch(&mut self) {
        self.revision += 1;
    }

    /// Changes whenever the content or component information changes (see
    /// [`Project::doc_fingerprint`]); computed once per change.
    pub fn fingerprint(&self) -> u64 {
        let mut cached = self.fingerprint.lock().unwrap_or_else(|e| e.into_inner());
        match *cached {
            Some((revision, f)) if revision == self.revision => f,
            _ => {
                let f = hash_serialized(&DocumentRef::of(self));
                *cached = Some((self.revision, f));
                f
            }
        }
    }

    pub fn panel(&self) -> &DocumentState {
        match &self.content {
            Content::Layout(p) => p,
        }
    }

    /// The document's state, to change: counted as a change (see
    /// [`Self::touch`]).
    pub fn panel_mut(&mut self) -> &mut DocumentState {
        self.touch();
        self.panel_mut_untracked()
    }

    /// The document's state, to change without counting a change: for the
    /// editor's frames, which notice changes themselves and
    /// [touch](Self::touch) the document when there are.
    pub fn panel_mut_untracked(&mut self) -> &mut DocumentState {
        match &mut self.content {
            Content::Layout(p) => p,
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.panel().layout
    }

    pub fn layout_mut(&mut self) -> &mut Layout {
        &mut self.panel_mut().layout
    }

    /// Physical size of one grid cell in micrometres.
    pub fn cell_um(&self) -> f64 {
        self.panel().cell_um
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
            .unwrap_or_else(|| super::names::initials(&self.name))
    }

    fn to_file(&self) -> DocumentFile {
        DocumentFile {
            id: self.id,
            name: self.name.clone(),
            component: self.component.clone(),
            cell_um: self.cell_um(),
            layout: self.layout().clone(),
        }
    }

    fn from_file(file: DocumentFile) -> Self {
        Self::new(file.id, file.name, file.component, Content::Layout(Box::new(DocumentState::with_layout(file.layout, file.cell_um))))
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
pub(crate) mod v5;
pub(crate) mod v6;

/// Format 3: layouts had no laser feeds (waveguides from a laser into a box).
pub(crate) mod v3 {
    use serde::{Deserialize, Serialize};

    use super::v4::{Instance, Layout4};
    use super::v6::Meta6 as ComponentMeta;
    use super::{DocId, DocumentFile};
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
            component: d.component.map(ComponentMeta::upgrade),
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
    use emath::{Pos2, Vec2};
    use serde::{Deserialize, Serialize};

    use super::v4::{Instance, Layout4};
    use super::v6::Meta6 as ComponentMeta;
    use super::{DocId, DocumentFile};
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
            component: d.component.map(ComponentMeta::upgrade),
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
    use emath::{Pos2, Vec2};
    use serde::{Deserialize, Serialize};

    use super::v6::Meta6 as ComponentMeta;
    use super::{DocId, DocumentFile};
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
                params: Vec::new(),
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
            component: d.component.map(ComponentMeta::upgrade),
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
    /// Parameters it exports, which each of its boxes can set.
    pub parameters: Vec<ComponentParam>,
    pub appearance: Appearance,
}

/// An exported parameter, as the boxes using a component see it.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentParam {
    pub name: String,
    pub unit: &'static str,
    pub min: f64,
    pub max: f64,
    /// Its value inside the component, used by boxes that don't set one.
    pub default: f64,
}

/// A layout's exported parameters, skipping any whose part or parameter is
/// gone.
pub fn component_parameters(layout: &Layout) -> Vec<ComponentParam> {
    use crate::panels::circuit::sweep;
    layout
        .exports
        .iter()
        .filter_map(|e| {
            let p = sweep::params(layout, e.item).into_iter().find(|p| p.name == e.param)?;
            Some(ComponentParam {
                name: e.name.clone(),
                unit: p.unit,
                min: p.min,
                max: p.max,
                default: sweep::get(layout, e.item, &e.param)?,
            })
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub entries: Vec<CatalogEntry>,
    /// The asset library's components (set by the workspace), placeable too.
    #[cfg(feature = "gui")]
    pub assets: std::sync::Arc<Vec<super::assets::AssetSummary>>,
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
    /// A component's colour, outline and picture; default for built-in blocks.
    pub appearance: Appearance,
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

    #[cfg(feature = "gui")]
    pub fn asset(&self, asset: super::assets::AssetRef) -> Option<&super::assets::AssetSummary> {
        self.assets.iter().find(|a| a.asset == asset)
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
        match block {
            BlockRef::Builtin(b) => b.port_counts(),
            BlockRef::Component(id) => self.get(*id).map(|e| e.interface.counts()).unwrap_or_default(),
        }
    }

    pub fn display(&self, block: &BlockRef) -> BlockDisplay {
        match block {
            BlockRef::Builtin(b) => {
                let info = b.info();
                let name = match b {
                    Builtin::InputPin { name } | Builtin::OutputPin { name } => format!("{} “{name}”", info.name),
                    Builtin::FunctionCell { name, .. } if !name.is_empty() => name.clone(),
                    Builtin::Model(m) => m.name.clone(),
                    _ => info.name.to_owned(),
                };
                BlockDisplay {
                    name,
                    initials: info.initials.to_owned(),
                    description: match b {
                        Builtin::Model(m) => m.summary(),
                        _ => info.description.to_owned(),
                    },
                    inputs: b.inputs(),
                    outputs: b.outputs(),
                    signal_inputs: b.signal_inputs(),
                    signal_outputs: b.signal_outputs(),
                    component: None,
                    appearance: Appearance::default(),
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
                    appearance: e.appearance.clone(),
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
                    appearance: Appearance::default(),
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
    /// The catalog, and the fingerprint it was made at: the editor asks for
    /// it every frame, and making it reads every document.
    catalog: std::sync::Mutex<Option<(u64, Catalog)>>,
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Documents for Project {
    fn layout(&self, id: DocId) -> Option<(&Layout, f64)> {
        self.doc(id).map(|d| (d.layout(), d.cell_um()))
    }

    fn name(&self, id: DocId) -> String {
        self.doc(id).map_or_else(|| "a deleted component".into(), |d| d.name.clone())
    }

    fn fingerprint(&self, id: DocId) -> Option<u64> {
        self.doc_fingerprint(id)
    }
}

impl Project {
    pub fn new() -> Self {
        Self::with_documents(Vec::new(), 1)
    }

    fn with_documents(documents: Vec<Document>, next_id: u64) -> Self {
        Self { documents, next_id, path: None, catalog: std::sync::Mutex::new(None) }
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
        self.documents.push(Document::new(id, name, None, Content::Layout(Box::new(DocumentState::with_layout(layout, cell_um)))));
        id
    }

    /// Add a layout already published as a component.
    pub fn add_component(&mut self, name: &str, initials: &str, description: &str, layout: Layout, cell_um: f64) -> DocId {
        let id = self.add_layout(name, layout, cell_um);
        self.doc_mut(id).expect("just added").component = Some(ComponentMeta {
            initials: initials.into(),
            description: description.into(),
            appearance: Appearance::default(),
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

    /// What every document is and uses, as boxes in the editor show them.
    /// Made again only after the project changes.
    pub fn catalog(&self) -> Catalog {
        let fingerprint = self.fingerprint();
        let mut cached = self.catalog.lock().unwrap_or_else(|e| e.into_inner());
        match &*cached {
            Some((f, catalog)) if *f == fingerprint => catalog.clone(),
            _ => {
                let catalog = self.make_catalog();
                *cached = Some((fingerprint, catalog.clone()));
                catalog
            }
        }
    }

    fn make_catalog(&self) -> Catalog {
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
                    parameters: component_parameters(d.layout()),
                    appearance: d.component.as_ref().map(|m| m.appearance.clone()).unwrap_or_default(),
                })
                .collect(),
            #[cfg(feature = "gui")]
            assets: Default::default(),
        }
    }

    /// Document `id` with every document it uses, as a project of its own
    /// with `id` first: what a component file or library asset holds.
    pub fn extract(&self, id: DocId) -> Option<Project> {
        let root = self.doc(id)?;
        let mut documents = vec![Document::from_file(root.to_file())];
        for dep in self.dependencies(id) {
            if let Some(d) = self.doc(dep) {
                documents.push(Document::from_file(d.to_file()));
            }
        }
        Some(Project::with_documents(documents, self.next_id))
    }

    /// Copy `bundle`'s first document, and those it uses, into this project;
    /// returns its id here. A component already here with the same name and
    /// content is reused rather than copied again.
    pub fn import(&mut self, bundle: &Project) -> Option<DocId> {
        let root = bundle.documents.first()?.id;
        let mut mapped = std::collections::HashMap::new();
        Some(self.import_doc(bundle, root, &mut mapped, 0))
    }

    fn import_doc(&mut self, bundle: &Project, id: DocId, mapped: &mut std::collections::HashMap<DocId, DocId>, depth: usize) -> DocId {
        if let Some(&here) = mapped.get(&id) {
            return here;
        }
        let Some(doc) = bundle.doc(id) else { return id };
        // What it uses comes first, so its boxes can point at the copies.
        if depth < 64 {
            for used in doc.references() {
                let here = self.import_doc(bundle, used, mapped, depth + 1);
                mapped.insert(used, here);
            }
        }
        let mut file = doc.to_file();
        for inst in &mut file.layout.instances {
            if let BlockRef::Component(old) = inst.block
                && let Some(&new) = mapped.get(&old)
            {
                inst.block = BlockRef::Component(new);
            }
        }
        let same = self.documents.iter().find(|d| {
            d.name == file.name && d.component == file.component && d.component.is_some() && *d.layout() == file.layout && d.cell_um() == file.cell_um
        });
        let here = match same {
            Some(d) => d.id,
            None => {
                file.id = self.allocate();
                file.name = self.unique_name(&file.name);
                let here = file.id;
                self.documents.push(Document::from_file(file));
                here
            }
        };
        mapped.insert(id, here);
        here
    }

    /// Settle a document's optics and electronics with the given register state.
    pub fn simulate(&self, id: DocId, machine: &Machine) -> Outcome {
        match self.doc(id) {
            Some(d) => sim::simulate(d.layout(), d.cell_um(), self, machine, Some(id), &Options::default()),
            None => Outcome::failed("no such document"),
        }
    }

    /// Changes whenever any document's content or component info changes,
    /// or documents come or go.
    pub fn fingerprint(&self) -> u64 {
        let documents: Vec<u64> = self.documents.iter().map(Document::fingerprint).collect();
        hash_serialized(&documents)
    }

    /// Changes whenever document `id`'s content or component info changes:
    /// what tells a tab it has unsaved changes. Computed again only after
    /// the document is changed (see [`Document::touch`]), so asking every
    /// frame costs nothing for a design of any size.
    pub fn doc_fingerprint(&self, id: DocId) -> Option<u64> {
        self.doc(id).map(Document::fingerprint)
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
            5 => {
                let old: v5::Project = file_format::decode_body(bytes)?;
                ProjectFile {
                    next_id: old.next_id,
                    documents: old.documents.into_iter().map(v5::upgrade).collect(),
                }
            }
            // Format 8 only added model blocks: format 7 reads as it is.
            7 => file_format::decode_body(bytes)?,
            6 => {
                let old: v6::Project = file_format::decode_body(bytes)?;
                ProjectFile {
                    next_id: old.next_id,
                    documents: old.documents.into_iter().map(v6::upgrade).collect(),
                }
            }
            _ => file_format::decode(bytes)?,
        };
        let documents: Vec<Document> = file.documents.into_iter().map(Document::from_file).collect();
        let next_id = documents.iter().map(|d| d.id.0 + 1).max().unwrap_or(1).max(file.next_id);
        Ok(Self::with_documents(documents, next_id))
    }

    /// Whether `bytes` are a Shine project file.
    pub fn is_project(bytes: &[u8]) -> bool {
        file_format::is_shine(bytes)
    }
}

#[cfg(test)]
mod tests;

/// A document as the file holds it, borrowed: what fingerprints hash.
#[derive(Serialize)]
struct DocumentRef<'a> {
    id: DocId,
    name: &'a str,
    component: &'a Option<ComponentMeta>,
    cell_um: f64,
    layout: &'a Layout,
}

impl<'a> DocumentRef<'a> {
    fn of(d: &'a Document) -> Self {
        Self { id: d.id, name: &d.name, component: &d.component, cell_um: d.cell_um(), layout: d.layout() }
    }
}

/// A fast hash of a value's serialisation: every frame asks, so it is
/// serialised straight into the hasher, without copying it or the bytes.
fn hash_serialized(value: &impl Serialize) -> u64 {
    use std::hash::BuildHasher;
    struct Hashing(foldhash::fast::FoldHasher<'static>);
    impl postcard::ser_flavors::Flavor for Hashing {
        type Output = u64;
        fn try_push(&mut self, b: u8) -> postcard::Result<()> {
            self.0.write_u8(b);
            Ok(())
        }
        fn try_extend(&mut self, bytes: &[u8]) -> postcard::Result<()> {
            self.0.write(bytes);
            Ok(())
        }
        fn finalize(self) -> postcard::Result<u64> {
            Ok(self.0.finish())
        }
    }
    let hasher = Hashing(foldhash::fast::FixedState::with_seed(0x5348_494e).build_hasher());
    postcard::serialize_with_flavor(value, hasher).expect("serialisable")
}
