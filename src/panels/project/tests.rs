use egui::{Pos2, Vec2};

use super::*;
use crate::panels::optics::examples;
use crate::panels::optics::model::{DETECTOR, OpticComponentKind, PlacedComponent, PortRef, cell_center};
use crate::panels::schematic::model::Schematic;

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
    let mut schematic = Schematic::default();
    let laser = schematic.add(
        BlockRef::Builtin(Builtin::Laser { amplitude: 1.0, phase: 0.0, wavelength_nm: 1550.0 }),
        Pos2::new(0.0, 0.0),
    );
    let det = schematic.add(BlockRef::Builtin(Builtin::Detector), Pos2::new(180.0, 120.0));
    schematic.connect(PortRef { instance: laser, port: 0 }, PortRef { instance: det, port: 0 });
    let old = v1::Project {
        next_id: 7,
        documents: vec![
            v1::Document {
                id: DocId(3),
                name: "Old layout".into(),
                component: Some(ComponentMeta { initials: "OL".into(), description: "d".into() }),
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
    assert_eq!(file_format::version(&p.to_bytes()), Ok(3));
}
