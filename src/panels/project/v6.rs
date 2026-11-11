//! Format 6: components had only initials and a description, no colour,
//! shape or icon. Frozen copies of the types that changed; formats 1–5
//! upgrade through these.

use serde::{Deserialize, Serialize};

use super::{ComponentMeta, DocId, DocumentFile};
use crate::panels::optics::model::Layout;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta6 {
    pub initials: String,
    pub description: String,
}

impl Meta6 {
    pub fn upgrade(self) -> ComponentMeta {
        ComponentMeta { initials: self.initials, description: self.description, ..Default::default() }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub name: String,
    pub component: Option<Meta6>,
    pub cell_um: f64,
    pub layout: Layout,
}

#[derive(Serialize, Deserialize)]
pub struct Project {
    pub next_id: u64,
    pub documents: Vec<Document>,
}

pub fn upgrade(d: Document) -> DocumentFile {
    DocumentFile { id: d.id, name: d.name, component: d.component.map(Meta6::upgrade), cell_um: d.cell_um, layout: d.layout }
}
