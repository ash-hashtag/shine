//! How long editor sweeps take, on designs of different kinds.
//!
//! `cargo run --release --example sweep_bench`

use std::time::Instant;

use egui::Vec2;
use shine::panels::circuit::sim::{Machine, NoDocuments};
use shine::panels::circuit::sweep::{self, SweepKind, SweepSpec};
use shine::panels::optics::examples;
use shine::panels::optics::model::{BlockRef, Builtin, Item, Layout, OpticComponentKind, PlacedComponent, PortRef, cell_center};
use shine::panels::project::Project;

fn time(name: &str, points: usize, run: impl Fn() -> sweep::SweepResult) {
    run(); // warm up
    let t = Instant::now();
    let rounds = 3;
    for _ in 0..rounds {
        let r = run();
        assert!(r.error.is_none(), "{name}: {:?}", r.error);
        assert_eq!(r.xs.len(), points);
    }
    let each = t.elapsed() / rounds;
    println!("{name:<44} {points:>5} points  {:>9.1} ms  {:>7.1} µs/point", each.as_secs_f64() * 1e3, each.as_secs_f64() * 1e6 / points as f64);
}

fn main() {
    let names = |id: shine::panels::optics::model::InstanceId| format!("box {}", id.0);

    // Components, rings and electronics: the WDM link example.
    let mut p = Project::new();
    let id = examples::wdm_link(&mut p);
    let wdm = p.doc(id).unwrap().layout().clone();
    let laser = wdm.components.iter().position(|c| matches!(c.kind, OpticComponentKind::Laser { .. })).unwrap();
    let spec = SweepSpec { kind: SweepKind::Wavelength { laser }, start: 1520.0, stop: 1570.0, points: 1001 };
    time("WDM link: wavelength", 1001, || sweep::run(&wdm, 10.0, &p, &Machine::default(), Some(id), &spec, &names));

    // A parameter sweep: the WDM link's first ring's coupling.
    let ring = wdm.instances.iter().find(|i| matches!(i.block, BlockRef::Builtin(Builtin::AddDropRing { .. }))).unwrap().id;
    let spec = SweepSpec { kind: SweepKind::Parameter { item: Item::Box(ring), param: "coupling" }, start: 0.01, stop: 0.3, points: 201 };
    time("WDM link: a ring's coupling", 201, || sweep::run(&wdm, 10.0, &p, &Machine::default(), Some(id), &spec, &names));

    // A big optical block: a laser into a 16-mode optical DFT.
    let mut l3 = Layout::with_components(vec![PlacedComponent {
        kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
        pos: cell_center(0, 0),
    }]);
    let dft = l3.add_instance(BlockRef::Builtin(Builtin::Dft { n: 16 }), (3, 0));
    l3.connect_feed(0, PortRef { instance: dft, port: 0 });
    let spec = SweepSpec { kind: SweepKind::Wavelength { laser: 0 }, start: 1500.0, stop: 1600.0, points: 2001 };
    time("16-mode optical DFT: wavelength", 2001, || sweep::run(&l3, 10.0, &NoDocuments, &Machine::default(), None, &spec, &names));
}
