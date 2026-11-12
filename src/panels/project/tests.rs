use emath::{Pos2, Vec2};

use super::*;
use crate::panels::optics::examples;
use crate::panels::optics::model::{DETECTOR, OpticComponentKind, PlacedComponent, PortRef, cell_center};
use crate::panels::optics::model::InstanceId;

fn part(i: i32, j: i32, kind: OpticComponentKind) -> PlacedComponent {
    PlacedComponent { kind, pos: cell_center(i, j) }
}

#[test]
fn interfaces_use_labels_and_default_names() {
    let mut l = Layout::with_components(vec![
        part(0, 0, OpticComponentKind::InputPort { dir: Vec2::new(1.0, 0.0) }),
        part(0, 2, OpticComponentKind::InputPort { dir: Vec2::new(1.0, 0.0) }),
        part(6, 1, OpticComponentKind::OutputPort),
        part(1, -2, OpticComponentKind::SignalIn),
        part(1, 4, OpticComponentKind::SignalOut),
    ]);
    l.set_label(0, "top");
    l.set_label(3, " clk ");
    l.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "pin".into() }), (6, 4));
    let i = layout_interface(&l);
    assert_eq!(i.inputs, vec!["top", "in2"]);
    assert_eq!(i.outputs, vec!["out1", "pin"]);
    assert_eq!(i.signal_inputs, vec!["clk"]);
    assert_eq!(i.signal_outputs, vec!["q1"]);
    assert_eq!(i.counts(), PortCounts { optical_in: 2, optical_out: 2, signal_in: 1, signal_out: 1 });
    assert!(!i.is_empty() && Interface::default().is_empty());
}

#[test]
fn catalog_describes_blocks_and_components() {
    let mut p = Project::new();
    let fa = examples::import_component(&mut p, 0).unwrap();
    let top = examples::eight_bit_adder(&mut p);
    assert_eq!(examples::import_component(&mut p, 0), Some(fa), "imported once");
    let catalog = p.catalog();
    assert_eq!(catalog.components().map(|e| e.id).collect::<Vec<_>>(), vec![fa]);
    assert!(catalog.get(top).unwrap().depends_on.contains(&fa));
    assert!(catalog.can_place(fa, top));
    assert!(!catalog.can_place(top, fa), "the adder uses the full adder");
    assert!(!catalog.can_place(fa, fa));
    assert!(!catalog.can_place(DocId(99), top));

    let d = catalog.display(&BlockRef::Component(fa));
    assert_eq!((d.initials.as_str(), d.component), ("FA", Some(fa)));
    assert_eq!(d.counts(), PortCounts { optical_in: 0, optical_out: 0, signal_in: 3, signal_out: 2 });
    assert_eq!(catalog.display(&BlockRef::Component(DocId(99))).name, "Missing component");
    let pin = catalog.display(&BlockRef::Builtin(Builtin::InputPin { name: "x".into() }));
    assert!(pin.name.contains("“x”"));
    let f = Builtin::electronic_palette().pop().unwrap();
    assert_eq!(catalog.display(&BlockRef::Builtin(f)).name, "Function");
    assert_eq!(catalog.counts(&BlockRef::Builtin(Builtin::Register { bits: 4 })).signal_in, 5);

    assert_eq!(p.users_of(fa), vec![top]);
    assert!(p.remove(fa).unwrap_err().contains("used in"));
    p.remove(top).unwrap();
    p.remove(fa).unwrap();
    assert!(p.documents.is_empty());
}

#[test]
fn names_initials_and_lookup() {
    let mut p = Project::new();
    let a = p.add_layout("Layout", Layout::default(), 10.0);
    let b = p.add_layout("Layout", Layout::default(), 10.0);
    assert_eq!(p.doc(b).unwrap().name, "Layout 2");
    assert_eq!(p.find("Layout 2"), Some(b));
    assert_eq!(p.doc(a).unwrap().initials(), "LA");
    let c = p.add_component("Beam router", "", "", Layout::default(), 10.0);
    assert_eq!(p.doc(c).unwrap().initials(), "BR", "falls back to the name");
    assert!(p.doc(c).unwrap().references().is_empty());
    assert_eq!(Project::default().documents.len(), 0);
}

#[test]
fn files_round_trip_and_fingerprints_follow_content() {
    let mut p = Project::new();
    examples::accumulator(&mut p);
    let bytes = p.to_bytes();
    assert!(Project::is_project(&bytes) && !Project::is_project(b"{}"));
    let q = Project::from_bytes(&bytes).unwrap();
    assert_eq!(q.to_bytes(), bytes);
    assert_eq!(q.fingerprint(), p.fingerprint());

    let mut r = Project::from_bytes(&bytes).unwrap();
    let id = r.documents[0].id;
    r.doc_mut(id).unwrap().panel_mut().cell_um = 11.0;
    assert_ne!(r.fingerprint(), p.fingerprint());
    // New documents never reuse an id.
    let fresh = r.add_layout("x", Layout::default(), 1.0);
    assert!(r.documents.iter().filter(|d| d.id == fresh).count() == 1);

    assert!(Project::from_bytes(&bytes[..bytes.len() - 3]).is_err());
    assert!(Project::from_bytes(b"nope").is_err());
}

#[test]
fn version_1_files_are_upgraded() {
    // Format 1 stored schematics with the blocks of the time (format 4's).
    let at = |id: u32, block: v4::Builtin, pos: Pos2| v4::SchematicInstance { id: InstanceId(id), block: v4::BlockRef::Builtin(block), pos };
    let schematic = v4::Schematic {
        instances: vec![
            at(0, v4::Builtin::Laser { amplitude: 1.0, phase: 0.0, wavelength_nm: 1550.0 }, Pos2::new(0.0, 0.0)),
            at(1, v4::Builtin::Detector, Pos2::new(180.0, 120.0)),
        ],
        wires: vec![crate::panels::optics::model::Wire {
            from: PortRef { instance: InstanceId(0), port: 0 },
            to: PortRef { instance: InstanceId(1), port: 0 },
            length_um: None,
        }],
        wire_settings: Default::default(),
        next_id: 2,
    };
    let old = v1::Project {
        next_id: 7,
        documents: vec![
            v1::Document {
                id: DocId(3),
                name: "Old layout".into(),
                component: Some(v6::Meta6 { initials: "OL".into(), description: "d".into() }),
                content: v1::Content::Layout {
                    cell_um: 2.5,
                    components: vec![
                        v1::Part {
                            kind: v1::Kind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 650.0, dir: Vec2::new(1.0, 0.0) },
                            pos: cell_center(0, 0),
                        },
                        v1::Part { kind: v1::Kind::Detector, pos: cell_center(3, 0) },
                    ],
                    lines: Vec::new(),
                },
            },
            v1::Document { id: DocId(5), name: "Old schematic".into(), component: None, content: v1::Content::Schematic { schematic } },
        ],
    };
    let mut bytes = file_format::encode(&old);
    bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
    let p = Project::from_bytes(&bytes).unwrap();
    assert_eq!(p.documents.len(), 2);
    let layout = p.doc(DocId(3)).unwrap();
    assert_eq!(layout.panel().cell_um, 2.5);
    assert_eq!(layout.layout().components[1].kind, DETECTOR);
    assert_eq!(layout.component.as_ref().unwrap().initials, "OL");
    let s = p.doc(DocId(5)).unwrap().layout();
    assert_eq!(s.instances.len(), 2);
    assert_eq!(s.instances[1].cell, (5, 3), "positions snap to cells");
    assert_eq!(s.wires.len(), 1);
    assert_eq!(s.next_instance, 2);
    // Both still simulate: the schematic's laser reaches its detector box.
    let o = p.simulate(DocId(5), &Machine::default());
    assert!(o.error.is_none(), "{:?}", o.error);
    assert!((o.wire_power(0) - 1.0).abs() < 1e-12);
    assert_eq!(p.simulate(DocId(3), &Machine::default()).reading(1), Some(1.0));
    assert!(p.simulate(DocId(42), &Machine::default()).error.is_some());
    // Saving writes the current version.
    assert_eq!(file_format::version(&p.to_bytes()), Ok(file_format::VERSION));
}

#[test]
fn version_2_lasers_keep_interfering() {
    let old = v2::Project {
        next_id: 2,
        documents: vec![v2::Document {
            id: DocId(1),
            name: "Two lasers".into(),
            component: None,
            cell_um: 10.0,
            layout: v2::Layout2 {
                components: vec![
                    v2::Part { kind: v2::Kind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 650.0, dir: Vec2::new(1.0, 0.0) }, pos: cell_center(0, 0) },
                    v2::Part { kind: v2::Kind::Detector { threshold: 0.5 }, pos: cell_center(3, 0) },
                    v2::Part { kind: v2::Kind::SignalOut, pos: cell_center(3, 2) },
                ],
                lines: Vec::new(),
                instances: Vec::new(),
                wires: Vec::new(),
                signals: Vec::new(),
                next_instance: 0,
                labels: vec![(2, "q".into())],
            },
        }],
    };
    let mut bytes = file_format::encode(&old);
    bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
    let p = Project::from_bytes(&bytes).unwrap();
    let l = p.doc(DocId(1)).unwrap().layout();
    assert!(matches!(l.components[0].kind, OpticComponentKind::Laser { source: 1, color: None, .. }), "on shared source A");
    assert_eq!(l.components[1].kind, OpticComponentKind::Detector { threshold: 0.5 });
    assert_eq!(l.label(2), Some("q"));
    assert_eq!(file_format::version(&p.to_bytes()), Ok(file_format::VERSION));
}

#[test]
fn version_3_files_gain_no_feeds() {
    let old = v3::Project {
        next_id: 2,
        documents: vec![v3::Document {
            id: DocId(1),
            name: "Format 3".into(),
            component: None,
            cell_um: 10.0,
            layout: v3::Layout3 {
                components: vec![part(0, 0, DETECTOR)],
                lines: Vec::new(),
                instances: Vec::new(),
                wires: Vec::new(),
                signals: Vec::new(),
                next_instance: 0,
                labels: vec![(0, "pd".into())],
            },
        }],
    };
    let mut bytes = file_format::encode(&old);
    bytes[4..6].copy_from_slice(&3u16.to_le_bytes());
    let p = Project::from_bytes(&bytes).unwrap();
    let l = p.doc(DocId(1)).unwrap().layout();
    assert_eq!((l.components.len(), l.label(0), l.feeds.len()), (1, Some("pd"), 0));
}

#[test]
fn version_4_waveguides_keep_their_spectra() {
    // Format 4's rings and delay lines had one index: they upgrade with
    // n_g = n_eff, so their spectra (away from the reference too) don't move.
    let ring = v4::Builtin::AddDropRing { coupling: 0.05, length_um: 12.9, n_eff: 2.4, loss_db_per_cm: 3.0 };
    let delay = v4::Builtin::DelayLine { length_um: 7.0, n_eff: 1.9 };
    let old = v4::Project {
        next_id: 2,
        documents: vec![v4::Document {
            id: DocId(1),
            name: "Format 4".into(),
            component: None,
            cell_um: 10.0,
            layout: v4::Layout4 {
                components: vec![part(0, 0, OpticComponentKind::Laser {
                    amplitude: 1.0, phase: 0.0, wavelength: 1530.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None,
                })],
                lines: Vec::new(),
                instances: vec![
                    v4::Instance { id: InstanceId(0), block: v4::BlockRef::Builtin(ring), cell: (2, 0) },
                    v4::Instance { id: InstanceId(1), block: v4::BlockRef::Builtin(delay), cell: (2, 6) },
                ],
                wires: Vec::new(),
                signals: Vec::new(),
                next_instance: 2,
                labels: Vec::new(),
                feeds: Vec::new(),
            },
        }],
    };
    let mut bytes = file_format::encode(&old);
    bytes[4..6].copy_from_slice(&4u16.to_le_bytes());
    let p = Project::from_bytes(&bytes).unwrap();
    let l = p.doc(DocId(1)).unwrap().layout();
    assert_eq!(
        l.instances[0].block,
        BlockRef::Builtin(Builtin::AddDropRing { coupling: 0.05, length_um: 12.9, n_eff: 2.4, loss_db_per_cm: 3.0, n_g: 2.4, reference_nm: 1550.0 })
    );
    assert_eq!(
        l.instances[1].block,
        BlockRef::Builtin(Builtin::DelayLine { length_um: 7.0, n_eff: 1.9, n_g: 1.9, reference_nm: 1550.0, loss_db_per_cm: 0.0 })
    );
    // The laser at 1530 nm, 20 nm from the reference, sees the same ring as before.
    let o = p.simulate(DocId(1), &Machine::default());
    let expected = crate::photonics::blocks::AddDropRing {
        coupling_in: 0.05, coupling_drop: 0.05, length_um: 12.9, n_eff: 2.4, loss_db_per_cm: 3.0, n_g: 2.4, reference_nm: 1234.0,
    };
    let mut c = crate::photonics::Circuit::new();
    let ports = crate::photonics::Block::build(&expected, &mut c).unwrap();
    let sol = c.simulate(1530.0, &[(ports.inputs[0], crate::photonics::C64::ONE)]).unwrap();
    for k in 0..2 {
        let want = sol.output_field(ports.outputs[k]).norm_sqr();
        assert!((o.port_power[&(InstanceId(0), k)] - want).abs() < 1e-9, "port {k}");
    }
}

#[test]
fn exports_and_instance_values_are_saved_and_format_5_files_open() {
    let mut p = Project::new();
    let mut inner = Layout::with_components(vec![part(0, 0, OpticComponentKind::PhaseShifter { phase: 0.0 })]);
    inner.exports.push(crate::panels::optics::model::Export { name: "phase".into(), item: Item::Part(0), param: "phase".into() });
    let comp = p.add_component("Shifter", "S", "", inner, 10.0);
    let mut top = Layout::default();
    let id = top.add_instance(BlockRef::Component(comp), (0, 0));
    top.instances[0].set_param("phase", Some(1.25));
    let top = p.add_layout("Top", top, 10.0);
    let q = Project::from_bytes(&p.to_bytes()).unwrap();
    assert_eq!(q.doc(comp).unwrap().layout().exports.len(), 1);
    assert_eq!(q.doc(top).unwrap().layout().instance(id).unwrap().param("phase"), Some(1.25));

    // A format-5 file has neither.
    let old = v5::Project {
        next_id: 2,
        documents: vec![v5::Document {
            id: DocId(1),
            name: "Format 5".into(),
            component: None,
            cell_um: 10.0,
            layout: v5::Layout5 {
                components: Vec::new(),
                lines: Vec::new(),
                instances: vec![v5::Instance { id: InstanceId(0), block: BlockRef::Builtin(Builtin::Detector), cell: (1, 1) }],
                wires: Vec::new(),
                signals: Vec::new(),
                next_instance: 1,
                labels: Vec::new(),
                feeds: Vec::new(),
            },
        }],
    };
    let mut bytes = file_format::encode(&old);
    bytes[4..6].copy_from_slice(&5u16.to_le_bytes());
    let l = Project::from_bytes(&bytes).unwrap().doc(DocId(1)).unwrap().layout().clone();
    assert!(l.exports.is_empty() && l.instances[0].params.is_empty() && l.instances[0].cell == (1, 1));
}

/// A component using another one: a box holding the example Full adder.
fn nested_project() -> (Project, DocId, DocId) {
    let mut p = Project::new();
    let fa = examples::import_component(&mut p, 0).unwrap();
    let mut outer = Layout::default();
    outer.add_instance(BlockRef::Component(fa), (0, 0));
    let wrapper = p.add_component("Two-bit thing", "TB", "Uses a full adder", outer, 10.0);
    (p, fa, wrapper)
}

#[test]
fn appearance_is_saved_and_format_6_still_opens() {
    use crate::panels::appearance::{BoxShape, Icon, IconFormat};
    let (mut p, fa, _) = nested_project();
    p.doc_mut(fa).unwrap().component.as_mut().unwrap().appearance = Appearance {
        color: Some([10, 200, 30]),
        shape: BoxShape::Chamfered,
        icon: Some(Icon::new(IconFormat::Svg, b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec())),
    };
    let back = Project::from_bytes(&p.to_bytes()).unwrap();
    assert_eq!(back.doc(fa).unwrap().component, p.doc(fa).unwrap().component);
    assert_eq!(back.catalog().get(fa).unwrap().appearance.shape, BoxShape::Chamfered);

    // The same project as format 6 wrote it: no appearance.
    let old = v6::Project {
        next_id: 9,
        documents: p
            .documents
            .iter()
            .map(|d| v6::Document {
                id: d.id,
                name: d.name.clone(),
                component: d.component.as_ref().map(|m| v6::Meta6 { initials: m.initials.clone(), description: m.description.clone() }),
                cell_um: d.panel().cell_um,
                layout: d.layout().clone(),
            })
            .collect(),
    };
    let mut bytes = file_format::MAGIC.to_vec();
    bytes.extend_from_slice(&6u16.to_le_bytes());
    bytes.extend(postcard::to_allocvec(&old).unwrap());
    let opened = Project::from_bytes(&bytes).unwrap();
    let meta = opened.doc(fa).unwrap().component.clone().unwrap();
    assert_eq!(meta.initials, "FA");
    assert_eq!(meta.appearance, Appearance::default());
}

#[test]
fn components_move_between_projects_with_what_they_use() {
    let (p, fa, wrapper) = nested_project();
    let bundle = p.extract(wrapper).unwrap();
    assert_eq!(bundle.documents.iter().map(|d| d.id).collect::<Vec<_>>(), vec![wrapper, fa], "itself first, then what it uses");

    // Into a project that already has a layout using ids 1 and 2.
    let mut other = Project::new();
    other.add_layout("Layout", Layout::default(), 10.0);
    other.add_layout("Another", Layout::default(), 10.0);
    let here = other.import(&bundle).unwrap();
    assert_eq!(other.documents.len(), 4);
    let inner = match other.doc(here).unwrap().layout().instances[0].block {
        BlockRef::Component(id) => id,
        _ => unreachable!(),
    };
    assert_eq!(other.doc(inner).unwrap().name, "Full adder", "the box points at the copied adder");
    let o = other.simulate(here, &Default::default());
    assert!(o.error.is_none(), "{:?}", o.error);

    // Again: nothing new.
    assert_eq!(other.import(&bundle), Some(here));
    assert_eq!(other.documents.len(), 4);

    // A different component of the same name is kept apart.
    let mut changed = p.extract(wrapper).unwrap();
    changed.doc_mut(wrapper).unwrap().component.as_mut().unwrap().description = "Changed".into();
    let second = other.import(&changed).unwrap();
    assert_ne!(second, here);
    assert_eq!(other.doc(second).unwrap().name, "Two-bit thing 2");
    assert_eq!(other.documents.len(), 5, "the unchanged adder is shared");
}

#[test]
fn a_waveguide_component_with_pins_works_inside_another_design() {
    // Its Output pin used to start a beam inside it as well, so placing it
    // failed with "already connected".
    let mut p = Project::new();
    let mut inner = Layout::default();
    let a = inner.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "a".into() }), (0, 0));
    let dc = inner.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.25 }), (3, 0));
    let bar = inner.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "bar".into() }), (7, 0));
    let cross = inner.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "cross".into() }), (7, 2));
    inner.connect_optical(PortRef { instance: a, port: 0 }, PortRef { instance: dc, port: 0 });
    inner.connect_optical(PortRef { instance: dc, port: 0 }, PortRef { instance: bar, port: 0 });
    inner.connect_optical(PortRef { instance: dc, port: 1 }, PortRef { instance: cross, port: 0 });
    let c = p.add_component("Tap", "T", "", inner, 10.0);
    let mut top = Layout::default();
    let laser = top.add_instance(BlockRef::Builtin(Builtin::Laser { amplitude: 1.0, phase: 0.0, wavelength_nm: 1550.0 }), (0, 0));
    let tap = top.add_instance(BlockRef::Component(c), (4, 0));
    top.connect_optical(PortRef { instance: laser, port: 0 }, PortRef { instance: tap, port: 0 });
    let id = p.add_layout("Top", top, 10.0);
    let o = p.simulate(id, &Default::default());
    assert!(o.error.is_none(), "{:?}", o.error);
    assert!((o.port_power[&(tap, 0)] - 0.75).abs() < 1e-9 && (o.port_power[&(tap, 1)] - 0.25).abs() < 1e-9, "{:?}", o.port_power);
}
