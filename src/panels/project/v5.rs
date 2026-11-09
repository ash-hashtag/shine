//! Format 5: no exported component parameters (and no per-instance values
//! for them). Frozen copies of the types that changed; formats 1–4 upgrade
//! through these.

use serde::{Deserialize, Serialize};

use super::{ComponentMeta, DocId, DocumentFile};
use crate::panels::optics::model::{self, BlockRef, Feed, InstanceId, Line, PlacedComponent, SignalWire, Wire};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub id: InstanceId,
    pub block: BlockRef,
    pub cell: (i32, i32),
}

impl Instance {
    pub fn upgrade(self) -> model::Instance {
        model::Instance { id: self.id, block: self.block, cell: self.cell, params: Vec::new() }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Layout5 {
    pub components: Vec<PlacedComponent>,
    pub lines: Vec<Line>,
    pub instances: Vec<Instance>,
    pub wires: Vec<Wire>,
    pub signals: Vec<SignalWire>,
    pub next_instance: u32,
    pub labels: Vec<(usize, String)>,
    pub feeds: Vec<Feed>,
}

impl Layout5 {
    pub fn upgrade(self) -> model::Layout {
        model::Layout {
            components: self.components,
            lines: self.lines,
            instances: self.instances.into_iter().map(Instance::upgrade).collect(),
            wires: self.wires,
            signals: self.signals,
            next_instance: self.next_instance,
            labels: self.labels,
            feeds: self.feeds,
            exports: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub name: String,
    pub component: Option<ComponentMeta>,
    pub cell_um: f64,
    pub layout: Layout5,
}

#[derive(Serialize, Deserialize)]
pub struct Project {
    pub next_id: u64,
    pub documents: Vec<Document>,
}

pub fn upgrade(d: Document) -> DocumentFile {
    DocumentFile { id: d.id, name: d.name, component: d.component, cell_um: d.cell_um, layout: d.layout.upgrade() }
}
