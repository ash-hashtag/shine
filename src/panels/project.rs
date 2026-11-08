//! A project: documents (free-space layouts and schematics), some of which are
//! published as reusable components.
//!
//! A component is just a document with a name, initials and a description.
//! Its ports come from the document itself: Input/Output port parts in a
//! layout, or input/output pins in a schematic, ordered top to bottom.
//! Instances refer to the document by id, so editing the document updates
//! every instance.

use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::beam_sim::{FreeSpaceBlock, port_order};
use super::file_format;
use super::optics::{Line, OpticsPanel, PlacedComponent, model::Layout};
use super::schematic::SchematicPanel;
use super::schematic::model::{BlockRef, Builtin, Schematic};
use super::schematic::sim::{self, Resolver, SchematicSim};
use crate::photonics::{Block, Circuit, Ports};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DocId(pub u64);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComponentMeta {
    pub initials: String,
    pub description: String,
}

pub enum Content {
    Layout(Box<OpticsPanel>),
    Schematic(Box<SchematicPanel>),
}

/// Port names of a document used as a component.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interface {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

pub struct Document {
    pub id: DocId,
    pub name: String,
    /// Set when the document is published as a component.
    pub component: Option<ComponentMeta>,
    pub content: Content,
}

impl Document {
    pub fn is_layout(&self) -> bool {
        matches!(self.content, Content::Layout(_))
    }

    pub fn interface(&self) -> Interface {
        match &self.content {
            Content::Layout(p) => {
                let (inputs, outputs) = port_order(&p.layout.components);
                Interface {
                    inputs: (1..=inputs.len()).map(|k| format!("in{k}")).collect(),
                    outputs: (1..=outputs.len()).map(|k| format!("out{k}")).collect(),
                }
            }
            Content::Schematic(p) => {
                let (inputs, outputs) = p.schematic.interface();
                Interface { inputs, outputs }
            }
        }
    }

    /// Documents this one uses directly.
    pub fn references(&self) -> Vec<DocId> {
        match &self.content {
            Content::Layout(_) => Vec::new(),
            Content::Schematic(p) => p.schematic.references(),
        }
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
            content: match &self.content {
                Content::Layout(p) => ContentFile::Layout {
                    cell_um: p.cell_um,
                    components: p.layout.components.clone(),
                    lines: p.layout.lines.clone(),
                },
                Content::Schematic(p) => ContentFile::Schematic {
                    schematic: p.schematic.clone(),
                },
            },
        }
    }

    fn from_file(file: DocumentFile) -> Self {
        let content = match file.content {
            ContentFile::Layout {
                cell_um,
                components,
                lines,
            } => Content::Layout(Box::new(OpticsPanel::with_layout(
                Layout { components, lines },
                cell_um,
            ))),
            ContentFile::Schematic { schematic } => {
                Content::Schematic(Box::new(SchematicPanel::with_schematic(schematic)))
            }
        };
        Self {
            id: file.id,
            name: file.name,
            component: file.component,
            content,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    next_id: u64,
    documents: Vec<DocumentFile>,
}

#[derive(Serialize, Deserialize)]
struct DocumentFile {
    id: DocId,
    name: String,
    #[serde(default)]
    component: Option<ComponentMeta>,
    content: ContentFile,
}

#[derive(Serialize, Deserialize)]
enum ContentFile {
    Layout {
        cell_um: f64,
        components: Vec<PlacedComponent>,
        lines: Vec<Line>,
    },
    Schematic {
        schematic: Schematic,
    },
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
    pub is_layout: bool,
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
    pub component: Option<DocId>,
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
        component != into
            && self
                .get(component)
                .is_some_and(|e| !e.depends_on.contains(&into))
    }

    pub fn display(&self, block: &BlockRef) -> BlockDisplay {
        match block {
            BlockRef::Builtin(b) => {
                let info = b.info();
                let name = match b {
                    Builtin::InputPin { name } | Builtin::OutputPin { name } => {
                        format!("{} “{name}”", info.name)
                    }
                    _ => info.name.to_owned(),
                };
                BlockDisplay {
                    name,
                    initials: info.initials.to_owned(),
                    description: info.description.to_owned(),
                    inputs: b.inputs(),
                    outputs: b.outputs(),
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
                    component: Some(*id),
                },
                None => BlockDisplay {
                    name: "Missing component".into(),
                    initials: "?".into(),
                    description: "This component was deleted.".into(),
                    inputs: Vec::new(),
                    outputs: Vec::new(),
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
        (2..)
            .map(|k| format!("{base} {k}"))
            .find(|n| !taken(n))
            .expect("unbounded")
    }

    pub fn add(&mut self, name: &str, content: Content) -> DocId {
        let id = self.allocate();
        let name = self.unique_name(name);
        self.documents.push(Document {
            id,
            name,
            component: None,
            content,
        });
        id
    }

    pub fn add_layout(&mut self, name: &str, layout: Layout, cell_um: f64) -> DocId {
        self.add(
            name,
            Content::Layout(Box::new(OpticsPanel::with_layout(layout, cell_um))),
        )
    }

    pub fn add_schematic(&mut self, name: &str, schematic: Schematic) -> DocId {
        self.add(
            name,
            Content::Schematic(Box::new(SchematicPanel::with_schematic(schematic))),
        )
    }

    pub fn doc(&self, id: DocId) -> Option<&Document> {
        self.documents.iter().find(|d| d.id == id)
    }

    pub fn doc_mut(&mut self, id: DocId) -> Option<&mut Document> {
        self.documents.iter_mut().find(|d| d.id == id)
    }

    /// Documents that use `id` directly.
    pub fn users_of(&self, id: DocId) -> Vec<DocId> {
        self.documents
            .iter()
            .filter(|d| d.references().contains(&id))
            .map(|d| d.id)
            .collect()
    }

    /// Delete a document unless another document uses it.
    pub fn remove(&mut self, id: DocId) -> Result<(), String> {
        let users = self.users_of(id);
        if let Some(user) = users.first().and_then(|&u| self.doc(u)) {
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
                    description: d
                        .component
                        .as_ref()
                        .map(|m| m.description.clone())
                        .unwrap_or_default(),
                    interface: d.interface(),
                    is_component: d.component.is_some(),
                    is_layout: d.is_layout(),
                    depends_on: self.dependencies(d.id),
                })
                .collect(),
        }
    }

    /// Simulate a schematic document with all the components it uses.
    pub fn simulate_schematic(&self, id: DocId) -> SchematicSim {
        match self.doc(id).map(|d| &d.content) {
            Some(Content::Schematic(p)) => sim::simulate(&p.schematic, self, &mut vec![id]),
            _ => SchematicSim::failed("not a schematic"),
        }
    }

    /// Changes whenever any document's content or component info changes.
    pub fn fingerprint(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for d in &self.documents {
            postcard::to_allocvec(&d.to_file())
                .expect("serialisable")
                .hash(&mut hasher);
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
        let file: ProjectFile = file_format::decode(bytes)?;
        let documents: Vec<Document> = file
            .documents
            .into_iter()
            .map(Document::from_file)
            .collect();
        let next_id = documents
            .iter()
            .map(|d| d.id.0 + 1)
            .max()
            .unwrap_or(1)
            .max(file.next_id);
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

impl Resolver for Project {
    fn build_component(
        &self,
        id: DocId,
        circuit: &mut Circuit,
        stack: &mut Vec<DocId>,
    ) -> Result<Ports, String> {
        let doc = self
            .doc(id)
            .ok_or_else(|| "A component used here was deleted".to_owned())?;
        if stack.contains(&id) {
            return Err(format!("“{}” contains itself", doc.name));
        }
        stack.push(id);
        let result = match &doc.content {
            Content::Layout(p) => FreeSpaceBlock {
                components: &p.layout.components,
                cell_um: p.cell_um,
            }
            .build(circuit)
            .map_err(|e| e.to_string()),
            Content::Schematic(p) => {
                sim::build_schematic(&p.schematic, self, circuit, stack).map(|b| b.ports)
            }
        };
        stack.pop();
        result.map_err(|e| {
            if e.starts_with('“') || e.starts_with("A component") {
                e
            } else {
                format!("In “{}”: {e}", doc.name)
            }
        })
    }
}

/// Port counts of every document, for quick lookups while drawing.
pub fn port_counts(catalog: &Catalog) -> HashMap<DocId, (usize, usize)> {
    catalog
        .entries
        .iter()
        .map(|e| (e.id, (e.interface.inputs.len(), e.interface.outputs.len())))
        .collect()
}

#[cfg(test)]
mod tests {
    use egui::Pos2;

    use super::*;
    use crate::panels::optics::model::cell_center;
    use crate::panels::optics::{OpticComponentKind, examples};
    use crate::panels::schematic::model::{InstanceId, PortRef};
    use crate::photonics::C64;

    fn b(block: Builtin) -> BlockRef {
        BlockRef::Builtin(block)
    }

    fn port(instance: InstanceId, port: usize) -> PortRef {
        PortRef { instance, port }
    }

    /// The free-space Mach–Zehnder example with its laser replaced by an
    /// input port and its detectors by output ports.
    fn free_space_mzi() -> Layout {
        let mut layout = examples::mach_zehnder();
        for c in &mut layout.components {
            c.kind = match c.kind {
                OpticComponentKind::Laser { dir, .. } => OpticComponentKind::InputPort { dir },
                OpticComponentKind::Detector => OpticComponentKind::OutputPort,
                other => other,
            };
        }
        layout
    }

    fn publish(project: &mut Project, id: DocId, initials: &str) {
        project.doc_mut(id).unwrap().component = Some(ComponentMeta {
            initials: initials.into(),
            description: "test component".into(),
        });
    }

    #[test]
    fn names_are_unique() {
        let mut p = Project::new();
        let a = p.add_layout("Layout", Layout::default(), 10.0);
        let b = p.add_layout("Layout", Layout::default(), 10.0);
        let c = p.add_schematic("Layout", Schematic::default());
        assert_eq!(p.doc(a).unwrap().name, "Layout");
        assert_eq!(p.doc(b).unwrap().name, "Layout 2");
        assert_eq!(p.doc(c).unwrap().name, "Layout 3");
        assert_ne!(a, b);
        assert!(p.doc(DocId(99)).is_none());
    }

    #[test]
    fn layout_interface_and_initials() {
        let mut p = Project::new();
        let id = p.add_layout("Free space MZI", free_space_mzi(), 10.0);
        let doc = p.doc(id).unwrap();
        assert_eq!(
            doc.interface(),
            Interface {
                inputs: vec!["in1".into()],
                outputs: vec!["out1".into(), "out2".into()]
            }
        );
        assert_eq!(
            doc.initials(),
            "FSM",
            "derived from the name until published"
        );
        publish(&mut p, id, "FZ");
        assert_eq!(p.doc(id).unwrap().initials(), "FZ");
        assert!(p.doc(id).unwrap().is_layout());
    }

    #[test]
    fn a_layout_component_behaves_like_the_layout() {
        let mut p = Project::new();
        let fs = p.add_layout("FS MZI", free_space_mzi(), 10.0);
        publish(&mut p, fs, "FZ");

        let mut s = Schematic::default();
        let laser = s.add(
            b(Builtin::Laser {
                amplitude: 1.0,
                phase: 0.0,
                wavelength_nm: 650.0,
            }),
            Pos2::ZERO,
        );
        let mzi = s.add(BlockRef::Component(fs), Pos2::new(100.0, 0.0));
        let d1 = s.add(b(Builtin::Detector), Pos2::new(200.0, -40.0));
        let d2 = s.add(b(Builtin::Detector), Pos2::new(200.0, 40.0));
        s.connect(port(laser, 0), port(mzi, 0));
        s.connect(port(mzi, 0), port(d1, 0));
        s.connect(port(mzi, 1), port(d2, 0));
        let top = p.add_schematic("Top", s);

        let sim = p.simulate_schematic(top);
        assert!(sim.error.is_none(), "{:?}", sim.error);
        let (r1, r2) = (sim.reading(d1).unwrap(), sim.reading(d2).unwrap());
        assert!((r1 + r2 - 1.0).abs() < 1e-9, "{r1} + {r2}");
        assert!(
            r1.max(r2) > 0.999999,
            "all light on one side, as in the layout example"
        );
    }

    #[test]
    fn schematic_components_nest() {
        let mut p = Project::new();
        // Inner: in -> attenuator (3 dB) -> out
        let mut inner = Schematic::default();
        let i = inner.add(b(Builtin::InputPin { name: "in".into() }), Pos2::ZERO);
        let a = inner.add(
            b(Builtin::Attenuator {
                loss_db: 10.0 * 2f64.log10(),
            }),
            Pos2::new(100.0, 0.0),
        );
        let o = inner.add(
            b(Builtin::OutputPin { name: "out".into() }),
            Pos2::new(200.0, 0.0),
        );
        inner.connect(port(i, 0), port(a, 0));
        inner.connect(port(a, 0), port(o, 0));
        let inner_id = p.add_schematic("Half", inner);
        publish(&mut p, inner_id, "½");

        // Middle: two halves in series.
        let mut middle = Schematic::default();
        let i = middle.add(b(Builtin::InputPin { name: "in".into() }), Pos2::ZERO);
        let h1 = middle.add(BlockRef::Component(inner_id), Pos2::new(100.0, 0.0));
        let h2 = middle.add(BlockRef::Component(inner_id), Pos2::new(200.0, 0.0));
        let o = middle.add(
            b(Builtin::OutputPin { name: "out".into() }),
            Pos2::new(300.0, 0.0),
        );
        middle.connect(port(i, 0), port(h1, 0));
        middle.connect(port(h1, 0), port(h2, 0));
        middle.connect(port(h2, 0), port(o, 0));
        let middle_id = p.add_schematic("Quarter", middle);
        publish(&mut p, middle_id, "¼");

        let mut top = Schematic::default();
        let laser = top.add(
            b(Builtin::Laser {
                amplitude: 2.0,
                phase: 0.0,
                wavelength_nm: 1550.0,
            }),
            Pos2::ZERO,
        );
        let q = top.add(BlockRef::Component(middle_id), Pos2::new(100.0, 0.0));
        let d = top.add(b(Builtin::Detector), Pos2::new(200.0, 0.0));
        top.connect(port(laser, 0), port(q, 0));
        top.connect(port(q, 0), port(d, 0));
        let top_id = p.add_schematic("Top", top);

        let sim = p.simulate_schematic(top_id);
        assert!(
            (sim.reading(d).unwrap() - 1.0).abs() < 1e-9,
            "4 W through a quarter: {:?}",
            sim
        );

        let catalog = p.catalog();
        assert_eq!(
            catalog.get(top_id).unwrap().depends_on,
            BTreeSet::from([inner_id, middle_id])
        );
        assert!(catalog.can_place(inner_id, top_id));
        assert!(
            !catalog.can_place(middle_id, inner_id),
            "would contain itself"
        );
        assert!(!catalog.can_place(inner_id, inner_id));
        assert!(!catalog.can_place(DocId(99), top_id));
        assert_eq!(catalog.components().count(), 2);
        assert_eq!(p.users_of(inner_id), vec![middle_id]);
        assert_eq!(p.remove(inner_id).unwrap_err(), "It is used in “Quarter”");
        assert!(p.remove(top_id).is_ok());
        assert!(p.remove(middle_id).is_ok());
        assert!(p.remove(inner_id).is_ok());
    }

    #[test]
    fn cycles_are_reported_not_followed() {
        let mut p = Project::new();
        let a = p.add_schematic("A", Schematic::default());
        let b_id = p.add_schematic("B", Schematic::default());
        let add_ref = |p: &mut Project, into: DocId, what: DocId| {
            if let Content::Schematic(s) = &mut p.doc_mut(into).unwrap().content {
                s.schematic.add(BlockRef::Component(what), Pos2::ZERO);
            }
        };
        add_ref(&mut p, a, b_id);
        add_ref(&mut p, b_id, a);
        let e = p.simulate_schematic(a).error.unwrap();
        assert!(e.contains("contains itself"), "{e}");
        // The catalog still terminates.
        assert!(p.catalog().get(a).unwrap().depends_on.contains(&a));
    }

    #[test]
    fn missing_components_and_non_schematics() {
        let mut p = Project::new();
        let mut s = Schematic::default();
        s.add(BlockRef::Component(DocId(42)), Pos2::ZERO);
        let id = p.add_schematic("S", s);
        assert!(p.simulate_schematic(id).error.unwrap().contains("deleted"));
        let layout = p.add_layout("L", Layout::default(), 10.0);
        assert_eq!(
            p.simulate_schematic(layout).error.as_deref(),
            Some("not a schematic")
        );
        let display = p.catalog().display(&BlockRef::Component(DocId(42)));
        assert_eq!(display.initials, "?");
    }

    #[test]
    fn display_of_builtins_and_components() {
        let mut p = Project::new();
        let id = p.add_layout("FS", free_space_mzi(), 10.0);
        publish(&mut p, id, "FZ");
        let catalog = p.catalog();
        let d = catalog.display(&BlockRef::Component(id));
        assert_eq!(
            (
                d.initials.as_str(),
                d.inputs.len(),
                d.outputs.len(),
                d.component
            ),
            ("FZ", 1, 2, Some(id))
        );
        let d = catalog.display(&b(Builtin::InputPin { name: "clk".into() }));
        assert!(d.name.contains("clk") && d.component.is_none());
        assert_eq!(port_counts(&catalog)[&id], (1, 2));
    }

    #[test]
    fn binary_round_trip_and_fingerprint() {
        let mut p = Project::new();
        let fs = p.add_layout("FS", free_space_mzi(), 7.5);
        publish(&mut p, fs, "FZ");
        let mut s = Schematic::default();
        s.add(BlockRef::Component(fs), Pos2::ZERO);
        for block in Builtin::palette() {
            s.add(b(block), Pos2::new(1.0, 2.0));
        }
        p.add_schematic("Top", s);
        let mut every_part = Layout::default();
        for (k, kind) in crate::panels::optics::OpticComponentKind::palette()
            .into_iter()
            .enumerate()
        {
            every_part.components.push(PlacedComponent {
                kind,
                pos: cell_center(k as i32, 0),
            });
        }
        every_part.lines.push(Line {
            start: cell_center(0, 0),
            end: cell_center(3, 3),
        });
        p.add_layout("Parts", every_part, 1.0);

        let bytes = p.to_bytes();
        assert!(Project::is_project(&bytes));
        assert!(!Project::is_project(br#"{"components": [], "lines": []}"#));
        let back = Project::from_bytes(&bytes).unwrap();
        assert_eq!(back.to_bytes(), bytes, "every block and part survives");
        assert_eq!(back.fingerprint(), p.fingerprint());
        let Content::Layout(l) = &back.doc(fs).unwrap().content else {
            panic!()
        };
        assert_eq!(l.cell_um, 7.5);

        // The same content as pretty JSON would be several times larger.
        let json = serde_json::to_vec_pretty(&ProjectFile {
            next_id: p.next_id,
            documents: p.documents.iter().map(Document::to_file).collect(),
        })
        .unwrap();
        assert!(
            bytes.len() * 3 < json.len(),
            "{} bytes vs {} bytes of JSON",
            bytes.len(),
            json.len()
        );

        // New ids never collide with loaded ones.
        let mut back = back;
        let new = back.add_layout("x", Layout::default(), 1.0);
        assert!(new.0 > fs.0 + 1);

        // Any edit changes the fingerprint.
        let before = back.fingerprint();
        if let Content::Layout(l) = &mut back.doc_mut(fs).unwrap().content {
            l.layout.components[0].pos = cell_center(-9, -9);
        }
        assert_ne!(back.fingerprint(), before);

        assert!(Project::from_bytes(b"{").is_err());
        assert!(
            Project::from_bytes(&bytes[..bytes.len() / 2])
                .err()
                .unwrap()
                .starts_with("damaged")
        );
    }

    #[test]
    fn resolver_errors_name_the_component() {
        let mut p = Project::new();
        let mut inner = Schematic::default();
        let i = inner.add(b(Builtin::InputPin { name: "in".into() }), Pos2::ZERO);
        inner.connect(port(i, 0), port(InstanceId(77), 0));
        let inner_id = p.add_schematic("Broken", inner);
        let mut c = Circuit::new();
        let e = p
            .build_component(inner_id, &mut c, &mut Vec::new())
            .unwrap_err();
        assert!(e.starts_with("In “Broken”:"), "{e}");
        let _ = C64::ONE;
    }
}
