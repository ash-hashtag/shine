//! Format 4: blocks without a group index or reference wavelength (and delay
//! lines without loss). These are the saved types as they were, frozen, so
//! older files keep decoding; they upgrade to blocks whose index doesn't
//! change with wavelength (`n_g = n_eff`), simulating exactly as before.
//! Formats 1–3 decode into these types too and upgrade through them (and on
//! through format 5's).

use egui::Pos2;
use serde::{Deserialize, Serialize};

use super::{ComponentMeta, DocId, DocumentFile};
use crate::panels::optics::model::{self, Feed, InstanceId, Line, PlacedComponent, SignalWire, Wire};
use crate::panels::schematic::model::WireSettings;
use crate::photonics::REFERENCE_NM;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Builtin {
    Laser { amplitude: f64, phase: f64, wavelength_nm: f64 },
    Detector,
    InputPin { name: String },
    OutputPin { name: String },
    Coupler { coupling: f64 },
    PhaseShifter { phase: f64 },
    Attenuator { loss_db: f64 },
    DelayLine { length_um: f64, n_eff: f64 },
    Mzi { theta: f64, phi: f64 },
    YSplitter,
    YCombiner,
    Crossing,
    Ring { coupling: f64, length_um: f64, n_eff: f64, loss_db_per_cm: f64 },
    AddDropRing { coupling: f64, length_um: f64, n_eff: f64, loss_db_per_cm: f64 },
    SplitterTree { levels: u32 },
    SwitchTree { levels: u32, address: u32 },
    Dft { n: u32 },
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
}

impl Builtin {
    pub fn upgrade(self) -> model::Builtin {
        use model::Builtin as B;
        match self {
            Self::Laser { amplitude, phase, wavelength_nm } => B::Laser { amplitude, phase, wavelength_nm },
            Self::Detector => B::Detector,
            Self::InputPin { name } => B::InputPin { name },
            Self::OutputPin { name } => B::OutputPin { name },
            Self::Coupler { coupling } => B::Coupler { coupling },
            Self::PhaseShifter { phase } => B::PhaseShifter { phase },
            Self::Attenuator { loss_db } => B::Attenuator { loss_db },
            Self::DelayLine { length_um, n_eff } => {
                B::DelayLine { length_um, n_eff, n_g: n_eff, reference_nm: REFERENCE_NM, loss_db_per_cm: 0.0 }
            }
            Self::Mzi { theta, phi } => B::Mzi { theta, phi },
            Self::YSplitter => B::YSplitter,
            Self::YCombiner => B::YCombiner,
            Self::Crossing => B::Crossing,
            Self::Ring { coupling, length_um, n_eff, loss_db_per_cm } => {
                B::Ring { coupling, length_um, n_eff, loss_db_per_cm, n_g: n_eff, reference_nm: REFERENCE_NM }
            }
            Self::AddDropRing { coupling, length_um, n_eff, loss_db_per_cm } => {
                B::AddDropRing { coupling, length_um, n_eff, loss_db_per_cm, n_g: n_eff, reference_nm: REFERENCE_NM }
            }
            Self::SplitterTree { levels } => B::SplitterTree { levels },
            Self::SwitchTree { levels, address } => B::SwitchTree { levels, address },
            Self::Dft { n } => B::Dft { n },
            Self::NumberInput { bits, value } => B::NumberInput { bits, value },
            Self::Display { bits } => B::Display { bits },
            Self::Register { bits } => B::Register { bits },
            Self::FunctionCell { name, inputs, light_inputs, outputs, light_outputs, wavelength_nm } => {
                B::FunctionCell { name, inputs, light_inputs, outputs, light_outputs, wavelength_nm }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BlockRef {
    Builtin(Builtin),
    Component(DocId),
}

impl BlockRef {
    pub fn upgrade(self) -> model::BlockRef {
        match self {
            Self::Builtin(b) => model::BlockRef::Builtin(b.upgrade()),
            Self::Component(id) => model::BlockRef::Component(id),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub id: InstanceId,
    pub block: BlockRef,
    pub cell: (i32, i32),
}

impl Instance {
    pub fn upgrade(self) -> super::v5::Instance {
        super::v5::Instance { id: self.id, block: self.block.upgrade(), cell: self.cell }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Layout4 {
    pub components: Vec<PlacedComponent>,
    pub lines: Vec<Line>,
    pub instances: Vec<Instance>,
    pub wires: Vec<Wire>,
    pub signals: Vec<SignalWire>,
    pub next_instance: u32,
    pub labels: Vec<(usize, String)>,
    pub feeds: Vec<Feed>,
}

impl Layout4 {
    pub fn upgrade(self) -> model::Layout {
        super::v5::Layout5 {
            components: self.components,
            lines: self.lines,
            instances: self.instances.into_iter().map(Instance::upgrade).collect(),
            wires: self.wires,
            signals: self.signals,
            next_instance: self.next_instance,
            labels: self.labels,
            feeds: self.feeds,
        }
        .upgrade()
    }
}

#[derive(Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub name: String,
    pub component: Option<ComponentMeta>,
    pub cell_um: f64,
    pub layout: Layout4,
}

#[derive(Serialize, Deserialize)]
pub struct Project {
    pub next_id: u64,
    pub documents: Vec<Document>,
}

pub fn upgrade(d: Document) -> DocumentFile {
    DocumentFile { id: d.id, name: d.name, component: d.component, cell_um: d.cell_um, layout: d.layout.upgrade() }
}

/// A box in a format-1 schematic, placed by position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchematicInstance {
    pub id: InstanceId,
    pub block: BlockRef,
    pub pos: Pos2,
}

/// A format-1 schematic document's content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Schematic {
    pub instances: Vec<SchematicInstance>,
    pub wires: Vec<Wire>,
    pub wire_settings: WireSettings,
    pub next_id: u32,
}

impl Schematic {
    pub fn upgrade(self) -> crate::panels::schematic::model::Schematic {
        let mut s = crate::panels::schematic::model::Schematic::default();
        s.wires = self.wires;
        s.wire_settings = self.wire_settings;
        for inst in self.instances {
            // Positions are already snapped; `add` keeps them and the ids stay in order.
            s.add(inst.block.upgrade(), inst.pos);
            s.instances.last_mut().expect("just added").id = inst.id;
        }
        s
    }
}
