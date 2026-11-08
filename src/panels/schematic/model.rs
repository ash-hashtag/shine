//! Schematic documents: blocks placed freely and wired with waveguides.

use egui::{Pos2, Vec2};
use serde::{Deserialize, Serialize};

use crate::panels::project::DocId;
use crate::photonics::blocks::{AddDropRing, AllPassRing, Dft, SplitterTree, SwitchTree};
use crate::photonics::{
    Attenuator, Block, Circuit, CircuitError, Crossing, Detector, DirectionalCoupler, Laser, Mzi,
    NodeId, PhaseShifter, Ports, Waveguide, YCombiner, YSplitter,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstanceId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    SourcesAndProbes,
    Pins,
    Elements,
    Circuits,
}

impl Category {
    pub fn title(self) -> &'static str {
        match self {
            Self::SourcesAndProbes => "Sources & probes",
            Self::Pins => "Component pins",
            Self::Elements => "Waveguide elements",
            Self::Circuits => "Circuits",
        }
    }
}

pub struct BlockInfo {
    pub name: &'static str,
    pub initials: &'static str,
    pub description: &'static str,
    pub category: Category,
}

/// Blocks that come with the editor. Each is backed by a `photonics` component or block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Builtin {
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
    },
    AddDropRing {
        coupling: f64,
        length_um: f64,
        n_eff: f64,
        loss_db_per_cm: f64,
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
}

fn numbered(prefix: &str, n: usize) -> Vec<String> {
    (1..=n).map(|k| format!("{prefix}{k}")).collect()
}

impl Builtin {
    /// Every built-in block with default parameters, in library order.
    pub fn palette() -> Vec<Builtin> {
        vec![
            Self::Laser {
                amplitude: 1.0,
                phase: 0.0,
                wavelength_nm: 1550.0,
            },
            Self::Detector,
            Self::InputPin { name: "in".into() },
            Self::OutputPin { name: "out".into() },
            Self::Coupler { coupling: 0.5 },
            Self::PhaseShifter { phase: 0.0 },
            Self::Attenuator { loss_db: 3.0 },
            Self::DelayLine {
                length_um: 10.0,
                n_eff: 2.4,
            },
            Self::Mzi {
                theta: std::f64::consts::PI,
                phi: 0.0,
            },
            Self::YSplitter,
            Self::YCombiner,
            Self::Crossing,
            Self::Ring {
                coupling: 0.05,
                length_um: 100.0,
                n_eff: 2.4,
                loss_db_per_cm: 10.0,
            },
            Self::AddDropRing {
                coupling: 0.05,
                length_um: 12.916_666_666_666_666,
                n_eff: 2.4,
                loss_db_per_cm: 0.0,
            },
            Self::SplitterTree { levels: 2 },
            Self::SwitchTree {
                levels: 2,
                address: 0,
            },
            Self::Dft { n: 4 },
        ]
    }

    pub fn info(&self) -> BlockInfo {
        use Category::*;
        let (name, initials, description, category) = match self {
            Self::Laser { .. } => (
                "Laser",
                "LS",
                "Coherent single-wavelength source.",
                SourcesAndProbes,
            ),
            Self::Detector => (
                "Detector",
                "PD",
                "Photodiode: absorbs light and reads its power.",
                SourcesAndProbes,
            ),
            Self::InputPin { .. } => (
                "Input pin",
                "IN",
                "Where light enters when this schematic is used as a component. Pins are ordered top to bottom.",
                Pins,
            ),
            Self::OutputPin { .. } => (
                "Output pin",
                "OUT",
                "Where light leaves when this schematic is used as a component. Shows the power passing through.",
                Pins,
            ),
            Self::Coupler { .. } => (
                "Directional coupler",
                "DC",
                "Two waveguides brought close: splits power between the bar and cross outputs.",
                Elements,
            ),
            Self::PhaseShifter { .. } => (
                "Phase shifter",
                "PS",
                "Heater or modulator adding a fixed phase.",
                Elements,
            ),
            Self::Attenuator { .. } => {
                ("Attenuator", "AT", "Wavelength-independent loss.", Elements)
            }
            Self::DelayLine { .. } => (
                "Delay line",
                "DL",
                "A waveguide of given length: phase depends on wavelength.",
                Elements,
            ),
            Self::Mzi { .. } => (
                "MZI switch",
                "MZI",
                "Mach–Zehnder interferometer. θ = π is the bar state, θ = 0 the cross state.",
                Elements,
            ),
            Self::YSplitter => (
                "Y-splitter",
                "Y",
                "Splits one waveguide into two with equal power.",
                Elements,
            ),
            Self::YCombiner => (
                "Y-combiner",
                "Y",
                "Merges two waveguides; only the in-phase part is guided.",
                Elements,
            ),
            Self::Crossing => (
                "Crossing",
                "X",
                "Two waveguides crossing without coupling.",
                Elements,
            ),
            Self::Ring { .. } => (
                "Ring resonator",
                "RR",
                "All-pass ring: notches out resonant wavelengths.",
                Circuits,
            ),
            Self::AddDropRing { .. } => (
                "Add–drop ring",
                "ADR",
                "Moves resonant wavelengths from the input bus to the drop port.",
                Circuits,
            ),
            Self::SplitterTree { .. } => (
                "Splitter tree",
                "1×N",
                "Binary tree of Y-splitters: equal fan-out.",
                Circuits,
            ),
            Self::SwitchTree { .. } => (
                "Switch tree",
                "SW",
                "Routes the input to the output selected by the address.",
                Circuits,
            ),
            Self::Dft { .. } => (
                "Optical DFT",
                "DFT",
                "MZI mesh computing the discrete Fourier transform.",
                Circuits,
            ),
        };
        BlockInfo {
            name,
            initials,
            description,
            category,
        }
    }

    pub fn inputs(&self) -> Vec<String> {
        match self {
            Self::Laser { .. } => Vec::new(),
            Self::InputPin { name } => vec![name.clone()],
            Self::Coupler { .. } | Self::Mzi { .. } | Self::Crossing => numbered("in", 2),
            Self::YCombiner => numbered("in", 2),
            Self::AddDropRing { .. } => vec!["in".into(), "add".into()],
            Self::Dft { n } => numbered("in", *n as usize),
            _ => vec!["in".into()],
        }
    }

    pub fn outputs(&self) -> Vec<String> {
        match self {
            Self::Detector => Vec::new(),
            Self::OutputPin { name } => vec![name.clone()],
            Self::Coupler { .. } | Self::Mzi { .. } | Self::Crossing | Self::YSplitter => {
                numbered("out", 2)
            }
            Self::AddDropRing { .. } => vec!["through".into(), "drop".into()],
            Self::Ring { .. } => vec!["through".into()],
            Self::SplitterTree { levels } | Self::SwitchTree { levels, .. } => {
                numbered("out", 1 << levels)
            }
            Self::Dft { n } => numbered("out", *n as usize),
            _ => vec!["out".into()],
        }
    }

    /// Add this block to `circuit`.
    pub fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
        let single = |node: NodeId, inputs: usize, outputs: usize| Ports {
            inputs: (0..inputs).map(|k| node.input(k)).collect(),
            outputs: (0..outputs).map(|k| node.output(k)).collect(),
        };
        Ok(match self {
            Self::Laser {
                amplitude,
                phase,
                wavelength_nm,
            } => {
                let n = c.add(Laser::new(*amplitude, *wavelength_nm).with_phase(*phase));
                single(n, 0, 1)
            }
            Self::Detector => {
                let n = c.add(Detector::new());
                single(n, 1, 0)
            }
            Self::InputPin { .. } | Self::OutputPin { .. } => {
                let n = c.add(PhaseShifter::new(0.0));
                single(n, 1, 1)
            }
            Self::Coupler { coupling } => {
                let n = c.add(DirectionalCoupler::new(*coupling));
                single(n, 2, 2)
            }
            Self::PhaseShifter { phase } => {
                let n = c.add(PhaseShifter::new(*phase));
                single(n, 1, 1)
            }
            Self::Attenuator { loss_db } => {
                let n = c.add(Attenuator::from_db(*loss_db));
                single(n, 1, 1)
            }
            Self::DelayLine { length_um, n_eff } => {
                let n = c.add(Waveguide::new(*length_um, *n_eff));
                single(n, 1, 1)
            }
            Self::Mzi { theta, phi } => {
                let n = c.add(Mzi::new(*theta, *phi));
                single(n, 2, 2)
            }
            Self::YSplitter => {
                let n = c.add(YSplitter);
                single(n, 1, 2)
            }
            Self::YCombiner => {
                let n = c.add(YCombiner);
                single(n, 2, 1)
            }
            Self::Crossing => {
                let n = c.add(Crossing);
                single(n, 2, 2)
            }
            Self::Ring {
                coupling,
                length_um,
                n_eff,
                loss_db_per_cm,
            } => AllPassRing {
                coupling: *coupling,
                length_um: *length_um,
                n_eff: *n_eff,
                loss_db_per_cm: *loss_db_per_cm,
            }
            .build(c)?,
            Self::AddDropRing {
                coupling,
                length_um,
                n_eff,
                loss_db_per_cm,
            } => AddDropRing {
                coupling_in: *coupling,
                coupling_drop: *coupling,
                length_um: *length_um,
                n_eff: *n_eff,
                loss_db_per_cm: *loss_db_per_cm,
            }
            .build(c)?,
            Self::SplitterTree { levels } => SplitterTree { levels: *levels }.build(c)?,
            Self::SwitchTree { levels, address } => SwitchTree {
                levels: *levels,
                address: *address as usize,
            }
            .build(c)?,
            Self::Dft { n } => Dft { n: *n as usize }.build(c)?,
        })
    }
}

/// What an instance is: a built-in block or another document used as a component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BlockRef {
    Builtin(Builtin),
    Component(DocId),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub id: InstanceId,
    pub block: BlockRef,
    /// Top-left corner in world units.
    pub pos: Pos2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortRef {
    pub instance: InstanceId,
    pub port: usize,
}

/// A waveguide from an output port to an input port.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub from: PortRef,
    pub to: PortRef,
    /// Fixed length in µm; `None` uses the document's wire model.
    #[serde(default)]
    pub length_um: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum WireModel {
    /// Lossless and phase-free: results do not depend on where blocks sit.
    Ideal,
    /// Length taken from the drawing, `um_per_unit` µm per world unit.
    Geometric { um_per_unit: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireSettings {
    pub model: WireModel,
    pub n_eff: f64,
    pub loss_db_per_cm: f64,
}

impl Default for WireSettings {
    fn default() -> Self {
        Self {
            model: WireModel::Ideal,
            n_eff: 2.4,
            loss_db_per_cm: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Schematic {
    pub instances: Vec<Instance>,
    pub wires: Vec<Wire>,
    #[serde(default)]
    pub wire_settings: WireSettings,
    next_id: u32,
}

/// World grid that instances snap to.
pub const SNAP: f32 = 10.0;

pub fn snap(p: Pos2) -> Pos2 {
    Pos2::new((p.x / SNAP).round() * SNAP, (p.y / SNAP).round() * SNAP)
}

impl Schematic {
    pub fn add(&mut self, block: BlockRef, pos: Pos2) -> InstanceId {
        let id = InstanceId(self.next_id);
        self.next_id += 1;
        self.instances.push(Instance {
            id,
            block,
            pos: snap(pos),
        });
        id
    }

    pub fn index_of(&self, id: InstanceId) -> Option<usize> {
        self.instances.iter().position(|i| i.id == id)
    }

    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    /// Connect an output to an input. Ports carry one waveguide each, so any
    /// wire already on either port is replaced.
    pub fn connect(&mut self, from: PortRef, to: PortRef) {
        self.wires.retain(|w| w.from != from && w.to != to);
        self.wires.push(Wire {
            from,
            to,
            length_um: None,
        });
    }

    pub fn wire_into(&self, to: PortRef) -> Option<usize> {
        self.wires.iter().position(|w| w.to == to)
    }

    pub fn wire_from(&self, from: PortRef) -> Option<usize> {
        self.wires.iter().position(|w| w.from == from)
    }

    /// Remove instances by index, with every wire touching them.
    pub fn remove(&mut self, indices: &[usize]) {
        let doomed: Vec<InstanceId> = indices
            .iter()
            .filter_map(|&i| self.instances.get(i))
            .map(|i| i.id)
            .collect();
        self.instances.retain(|i| !doomed.contains(&i.id));
        self.wires
            .retain(|w| !doomed.contains(&w.from.instance) && !doomed.contains(&w.to.instance));
    }

    /// Copy instances (and the wires between them), offset by `offset`.
    /// Returns the indices of the copies.
    pub fn duplicate(&mut self, indices: &[usize], offset: Vec2) -> Vec<usize> {
        let mut mapping = Vec::new();
        for &i in indices {
            let Some(original) = self.instances.get(i).cloned() else {
                continue;
            };
            let id = self.add(original.block, original.pos + offset);
            mapping.push((original.id, id));
        }
        let map = |id: InstanceId| mapping.iter().find(|m| m.0 == id).map(|m| m.1);
        let copies: Vec<Wire> = self
            .wires
            .iter()
            .filter_map(|w| {
                Some(Wire {
                    from: PortRef {
                        instance: map(w.from.instance)?,
                        port: w.from.port,
                    },
                    to: PortRef {
                        instance: map(w.to.instance)?,
                        port: w.to.port,
                    },
                    length_um: w.length_um,
                })
            })
            .collect();
        self.wires.extend(copies);
        let first = self.instances.len() - mapping.len();
        (first..self.instances.len()).collect()
    }

    /// Indices of input and output pins, ordered top to bottom, then left to right.
    pub fn pin_order(&self) -> (Vec<usize>, Vec<usize>) {
        let sorted = |want_input: bool| {
            let mut v: Vec<usize> = (0..self.instances.len())
                .filter(|&i| match &self.instances[i].block {
                    BlockRef::Builtin(Builtin::InputPin { .. }) => want_input,
                    BlockRef::Builtin(Builtin::OutputPin { .. }) => !want_input,
                    _ => false,
                })
                .collect();
            v.sort_by(|&a, &b| {
                let (pa, pb) = (self.instances[a].pos, self.instances[b].pos);
                pa.y.total_cmp(&pb.y).then(pa.x.total_cmp(&pb.x))
            });
            v
        };
        (sorted(true), sorted(false))
    }

    /// Port names exposed when this schematic is used as a component.
    pub fn interface(&self) -> (Vec<String>, Vec<String>) {
        let (inputs, outputs) = self.pin_order();
        let name = |i: usize| match &self.instances[i].block {
            BlockRef::Builtin(Builtin::InputPin { name } | Builtin::OutputPin { name }) => {
                name.clone()
            }
            _ => unreachable!("pin_order only returns pins"),
        };
        (
            inputs.into_iter().map(name).collect(),
            outputs.into_iter().map(name).collect(),
        )
    }

    /// Documents used directly by this schematic.
    pub fn references(&self) -> Vec<DocId> {
        let mut refs: Vec<DocId> = self
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photonics::C64;

    fn builtin(b: Builtin) -> BlockRef {
        BlockRef::Builtin(b)
    }

    #[test]
    fn every_builtin_builds_with_its_declared_ports() {
        let mut names = std::collections::HashSet::new();
        for b in Builtin::palette() {
            let info = b.info();
            assert!(!info.description.is_empty() && !info.initials.is_empty());
            assert!(names.insert(info.name), "duplicate name {}", info.name);
            let mut c = Circuit::new();
            let ports = b.build(&mut c).unwrap();
            assert_eq!(ports.inputs.len(), b.inputs().len(), "{}", info.name);
            assert_eq!(ports.outputs.len(), b.outputs().len(), "{}", info.name);
            for o in &ports.outputs {
                assert!(
                    c.sink_of(*o).is_none(),
                    "{}: outputs must be open",
                    info.name
                );
            }
        }
    }

    #[test]
    fn categories_have_titles() {
        for b in Builtin::palette() {
            assert!(!b.info().category.title().is_empty());
        }
    }

    #[test]
    fn default_add_drop_ring_is_resonant_at_1550() {
        let b = &Builtin::palette()[13];
        let mut c = Circuit::new();
        let p = b.build(&mut c).unwrap();
        let sol = c.simulate(1550.0, &[(p.inputs[0], C64::ONE)]).unwrap();
        assert!(sol.output_field(p.outputs[1]).norm_sqr() > 0.99);
    }

    #[test]
    fn pins_are_passthroughs() {
        let mut c = Circuit::new();
        let p = Builtin::InputPin { name: "a".into() }
            .build(&mut c)
            .unwrap();
        let sol = c
            .simulate(1550.0, &[(p.inputs[0], C64::new(0.0, 2.0))])
            .unwrap();
        assert_eq!(sol.output_field(p.outputs[0]), C64::new(0.0, 2.0));
    }

    #[test]
    fn add_snaps_and_assigns_unique_ids() {
        let mut s = Schematic::default();
        let a = s.add(builtin(Builtin::Detector), Pos2::new(13.0, 26.0));
        let b = s.add(builtin(Builtin::Detector), Pos2::ZERO);
        assert_ne!(a, b);
        assert_eq!(s.instance(a).unwrap().pos, Pos2::new(10.0, 30.0));
        assert_eq!(s.index_of(b), Some(1));
        assert_eq!(s.index_of(InstanceId(99)), None);
        assert_eq!(snap(Pos2::new(-4.0, 5.0)), Pos2::new(-0.0, 10.0));
    }

    #[test]
    fn connect_replaces_wires_on_either_port() {
        let mut s = Schematic::default();
        let a = s.add(builtin(Builtin::YSplitter), Pos2::ZERO);
        let b = s.add(builtin(Builtin::YCombiner), Pos2::ZERO);
        let out = |i, p| PortRef {
            instance: i,
            port: p,
        };
        s.connect(out(a, 0), out(b, 0));
        s.connect(out(a, 1), out(b, 1));
        assert_eq!(s.wires.len(), 2);
        // Re-using a's output 0 replaces its wire.
        s.connect(out(a, 0), out(b, 1));
        assert_eq!(s.wires.len(), 1);
        assert_eq!(s.wire_into(out(b, 1)), Some(0));
        assert_eq!(s.wire_from(out(a, 0)), Some(0));
        assert_eq!(s.wire_into(out(b, 0)), None);
    }

    #[test]
    fn remove_drops_attached_wires() {
        let mut s = Schematic::default();
        let a = s.add(builtin(Builtin::YSplitter), Pos2::ZERO);
        let b = s.add(builtin(Builtin::YCombiner), Pos2::ZERO);
        let c = s.add(builtin(Builtin::Detector), Pos2::ZERO);
        s.connect(
            PortRef {
                instance: a,
                port: 0,
            },
            PortRef {
                instance: b,
                port: 0,
            },
        );
        s.connect(
            PortRef {
                instance: b,
                port: 0,
            },
            PortRef {
                instance: c,
                port: 0,
            },
        );
        s.remove(&[1, 7]);
        assert_eq!(s.instances.len(), 2);
        assert!(s.wires.is_empty());
    }

    #[test]
    fn duplicate_copies_internal_wires_only() {
        let mut s = Schematic::default();
        let a = s.add(builtin(Builtin::YSplitter), Pos2::ZERO);
        let b = s.add(builtin(Builtin::YCombiner), Pos2::new(100.0, 0.0));
        let c = s.add(builtin(Builtin::Detector), Pos2::new(200.0, 0.0));
        s.connect(
            PortRef {
                instance: a,
                port: 0,
            },
            PortRef {
                instance: b,
                port: 0,
            },
        );
        s.connect(
            PortRef {
                instance: b,
                port: 0,
            },
            PortRef {
                instance: c,
                port: 0,
            },
        );
        s.wires[0].length_um = Some(5.0);
        let copies = s.duplicate(&[0, 1, 42], Vec2::new(0.0, 50.0));
        assert_eq!(copies, vec![3, 4]);
        assert_eq!(s.instances[3].pos, Pos2::new(0.0, 50.0));
        assert_eq!(
            s.wires.len(),
            3,
            "only the a->b wire is internal to the copy"
        );
        assert_eq!(s.wires[2].length_um, Some(5.0));
        assert_eq!(s.wires[2].from.instance, s.instances[3].id);
    }

    #[test]
    fn pins_define_the_interface_top_to_bottom() {
        let mut s = Schematic::default();
        s.add(
            builtin(Builtin::InputPin {
                name: "lower".into(),
            }),
            Pos2::new(0.0, 100.0),
        );
        s.add(
            builtin(Builtin::InputPin {
                name: "upper".into(),
            }),
            Pos2::new(0.0, 0.0),
        );
        s.add(
            builtin(Builtin::OutputPin { name: "y".into() }),
            Pos2::new(300.0, 50.0),
        );
        s.add(
            builtin(Builtin::Coupler { coupling: 0.5 }),
            Pos2::new(100.0, 0.0),
        );
        let (inputs, outputs) = s.interface();
        assert_eq!(inputs, vec!["upper", "lower"]);
        assert_eq!(outputs, vec!["y"]);
    }

    #[test]
    fn references_are_unique() {
        let mut s = Schematic::default();
        s.add(BlockRef::Component(DocId(3)), Pos2::ZERO);
        s.add(BlockRef::Component(DocId(1)), Pos2::ZERO);
        s.add(BlockRef::Component(DocId(3)), Pos2::ZERO);
        s.add(builtin(Builtin::Detector), Pos2::ZERO);
        assert_eq!(s.references(), vec![DocId(1), DocId(3)]);
    }

    #[test]
    fn serde_round_trip() {
        let mut s = Schematic::default();
        let a = s.add(
            builtin(Builtin::Laser {
                amplitude: 1.0,
                phase: 0.5,
                wavelength_nm: 1310.0,
            }),
            Pos2::ZERO,
        );
        let b = s.add(BlockRef::Component(DocId(7)), Pos2::new(100.0, 0.0));
        s.connect(
            PortRef {
                instance: a,
                port: 0,
            },
            PortRef {
                instance: b,
                port: 0,
            },
        );
        s.wire_settings.model = WireModel::Geometric { um_per_unit: 0.5 };
        let back: Schematic = postcard::from_bytes(&postcard::to_allocvec(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        // Ids keep increasing after a reload.
        let mut back = back;
        assert_eq!(
            back.add(builtin(Builtin::Detector), Pos2::ZERO),
            InstanceId(2)
        );
    }
}
