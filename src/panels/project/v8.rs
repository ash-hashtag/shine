//! Format 8, the last in postcard (Shine 0.1): frozen copies of the types
//! files of formats 5 to 8 hold, as they were then, so the model can change
//! (files are protobuf now, `crates/shine-format`). Formats 1–4 upgrade
//! through these too.

use serde::{Deserialize, Serialize};

use super::{ComponentMeta, DocId, DocumentFile};
use crate::panels::appearance::Appearance;

/// A component's information as format 8 had it (no alias).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta8 {
    pub initials: String,
    pub description: String,
    pub appearance: Appearance,
}

impl Meta8 {
    pub fn upgrade(self) -> ComponentMeta {
        ComponentMeta { initials: self.initials, description: self.description, appearance: self.appearance, alias: None }
    }
}
use crate::panels::optics::model::{self, Export, Feed, InstanceId, Line, PlacedComponent, SignalWire, Wire};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Builtin8 {
    Laser {
        amplitude: f64,
        phase: f64,
        wavelength_nm: f64,
    },
    Detector,
    InputPin {
        name: String,
    },
    OutputPin {
        name: String,
    },
    Coupler {
        coupling: f64,
    },
    PhaseShifter {
        phase: f64,
    },
    Attenuator {
        loss_db: f64,
    },
    DelayLine {
        length_um: f64,
        n_eff: f64,
        n_g: f64,
        reference_nm: f64,
        loss_db_per_cm: f64,
    },
    Mzi {
        theta: f64,
        phi: f64,
    },
    YSplitter,
    YCombiner,
    Crossing,
    Ring {
        coupling: f64,
        length_um: f64,
        n_eff: f64,
        loss_db_per_cm: f64,
        n_g: f64,
        reference_nm: f64,
    },
    AddDropRing {
        coupling: f64,
        length_um: f64,
        n_eff: f64,
        loss_db_per_cm: f64,
        n_g: f64,
        reference_nm: f64,
    },
    SplitterTree {
        levels: u32,
    },
    SwitchTree {
        levels: u32,
        address: u32,
    },
    Dft {
        n: u32,
    },
    NumberInput { bits: u32, value: u64 },
    Display { bits: u32 },
    Register { bits: u32 },
    FunctionCell {
        name: String,
        inputs: u32,
        light_inputs: u32,
        outputs: Vec<String>,
        light_outputs: Vec<String>,
        wavelength_nm: f64,
    },
    Model(std::sync::Arc<crate::panels::models::ModelSpec>),
    Value { name: String, value: f64 },
}

impl Builtin8 {
    pub fn upgrade(self) -> model::Builtin {
        use Builtin8 as B;
        match self {
            B::Laser { amplitude, phase, wavelength_nm } => model::Builtin::Laser { amplitude, phase, wavelength_nm },
            B::Detector => model::Builtin::Detector,
            B::InputPin { name } => model::Builtin::InputPin { name },
            B::OutputPin { name } => model::Builtin::OutputPin { name },
            B::Coupler { coupling } => model::Builtin::Coupler { coupling },
            B::PhaseShifter { phase } => model::Builtin::PhaseShifter { phase },
            B::Attenuator { loss_db } => model::Builtin::Attenuator { loss_db },
            B::DelayLine { length_um, n_eff, n_g, reference_nm, loss_db_per_cm } => model::Builtin::DelayLine { length_um, n_eff, n_g, reference_nm, loss_db_per_cm },
            B::Mzi { theta, phi } => model::Builtin::mzi(theta, phi),
            B::YSplitter => model::Builtin::YSplitter,
            B::YCombiner => model::Builtin::YCombiner,
            B::Crossing => model::Builtin::Crossing,
            B::Ring { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm } => model::Builtin::Ring { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm },
            B::AddDropRing { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm } => model::Builtin::AddDropRing { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm },
            B::SplitterTree { levels } => model::Builtin::SplitterTree { levels },
            B::SwitchTree { levels, address } => model::Builtin::SwitchTree { levels, address },
            B::Dft { n } => model::Builtin::Dft { n },
            B::NumberInput { bits, value } => model::Builtin::NumberInput { bits, value },
            B::Display { bits } => model::Builtin::Display { bits },
            B::Register { bits } => model::Builtin::Register { bits },
            B::FunctionCell { name, inputs, light_inputs, outputs, light_outputs, wavelength_nm } => model::Builtin::FunctionCell { name, inputs, light_inputs, outputs, light_outputs, wavelength_nm },
            B::Model(x) => model::Builtin::Model(x),
            B::Value { name, value } => model::Builtin::Value { name, value },
        }
    }
}

#[cfg(test)]
impl Builtin8 {
    /// `b` as format 8 had it (MZI arms dropped): for tests making old files.
    pub fn downgrade(b: model::Builtin) -> Self {
        use Builtin8 as B;
        match b {
            model::Builtin::Laser { amplitude, phase, wavelength_nm } => B::Laser { amplitude, phase, wavelength_nm },
            model::Builtin::Detector => B::Detector,
            model::Builtin::InputPin { name } => B::InputPin { name },
            model::Builtin::OutputPin { name } => B::OutputPin { name },
            model::Builtin::Coupler { coupling } => B::Coupler { coupling },
            model::Builtin::PhaseShifter { phase } => B::PhaseShifter { phase },
            model::Builtin::Attenuator { loss_db } => B::Attenuator { loss_db },
            model::Builtin::DelayLine { length_um, n_eff, n_g, reference_nm, loss_db_per_cm } => B::DelayLine { length_um, n_eff, n_g, reference_nm, loss_db_per_cm },
            model::Builtin::Mzi { theta, phi, .. } => B::Mzi { theta, phi },
            model::Builtin::YSplitter => B::YSplitter,
            model::Builtin::YCombiner => B::YCombiner,
            model::Builtin::Crossing => B::Crossing,
            model::Builtin::Ring { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm } => B::Ring { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm },
            model::Builtin::AddDropRing { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm } => B::AddDropRing { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm },
            model::Builtin::SplitterTree { levels } => B::SplitterTree { levels },
            model::Builtin::SwitchTree { levels, address } => B::SwitchTree { levels, address },
            model::Builtin::Dft { n } => B::Dft { n },
            model::Builtin::NumberInput { bits, value } => B::NumberInput { bits, value },
            model::Builtin::Display { bits } => B::Display { bits },
            model::Builtin::Register { bits } => B::Register { bits },
            model::Builtin::FunctionCell { name, inputs, light_inputs, outputs, light_outputs, wavelength_nm } => B::FunctionCell { name, inputs, light_inputs, outputs, light_outputs, wavelength_nm },
            model::Builtin::Model(x) => B::Model(x),
            model::Builtin::Value { name, value } => B::Value { name, value },
        }
    }
}

#[cfg(test)]
impl Layout8 {
    /// `l` as format 8 had it: for tests making old files.
    pub fn downgrade(l: model::Layout) -> Self {
        Self {
            components: l.components,
            lines: l.lines,
            instances: l
                .instances
                .into_iter()
                .map(|i| Instance8 {
                    id: i.id,
                    block: match i.block {
                        model::BlockRef::Builtin(b) => BlockRef8::Builtin(Builtin8::downgrade(b)),
                        model::BlockRef::Component(id) => BlockRef8::Component(id),
                    },
                    cell: i.cell,
                    params: i.params,
                })
                .collect(),
            wires: l.wires,
            signals: l.signals,
            next_instance: l.next_instance,
            labels: l.labels,
            feeds: l.feeds,
            exports: l.exports,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BlockRef8 {
    Builtin(Builtin8),
    Component(DocId),
}

impl BlockRef8 {
    pub fn upgrade(self) -> model::BlockRef {
        match self {
            Self::Builtin(b) => model::BlockRef::Builtin(b.upgrade()),
            Self::Component(id) => model::BlockRef::Component(id),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Instance8 {
    pub id: InstanceId,
    pub block: BlockRef8,
    pub cell: (i32, i32),
    pub params: Vec<(String, f64)>,
}

#[derive(Serialize, Deserialize)]
pub struct Layout8 {
    pub components: Vec<PlacedComponent>,
    pub lines: Vec<Line>,
    pub instances: Vec<Instance8>,
    pub wires: Vec<Wire>,
    pub signals: Vec<SignalWire>,
    pub next_instance: u32,
    pub labels: Vec<(usize, String)>,
    pub feeds: Vec<Feed>,
    pub exports: Vec<Export>,
}

impl Layout8 {
    pub fn upgrade(self) -> model::Layout {
        model::Layout {
            components: self.components,
            lines: self.lines,
            instances: self
                .instances
                .into_iter()
                .map(|i| model::Instance { id: i.id, block: i.block.upgrade(), cell: i.cell, params: i.params, simulate: Default::default() })
                .collect(),
            wires: self.wires,
            signals: self.signals,
            next_instance: self.next_instance,
            labels: self.labels,
            feeds: self.feeds,
            exports: self.exports,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub name: String,
    pub component: Option<Meta8>,
    pub cell_um: f64,
    pub layout: Layout8,
}

#[derive(Serialize, Deserialize)]
pub struct Project {
    pub next_id: u64,
    pub documents: Vec<Document>,
}

pub fn upgrade(d: Document) -> DocumentFile {
    DocumentFile { id: d.id, name: d.name, component: d.component.map(Meta8::upgrade), cell_um: d.cell_um, layout: d.layout.upgrade() }
}
