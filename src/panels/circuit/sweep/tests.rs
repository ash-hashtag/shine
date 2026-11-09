use egui::Vec2;

use super::*;
use crate::panels::circuit::sim::NoDocuments;
use crate::panels::optics::examples;
use crate::panels::optics::model::{PlacedComponent, cell_center};
use crate::panels::project::Project;

fn no_names(id: InstanceId) -> String {
    format!("box {}", id.0)
}

#[test]
fn parameter_sweep_steers_an_interferometer() {
    // Sweeping the free-space MZI's phase shifter moves the light between
    // its two detectors as cos² and sin²; their sum stays 1.
    let l = examples::mach_zehnder();
    let shifter = l.components.iter().position(|c| matches!(c.kind, OpticComponentKind::PhaseShifter { .. })).unwrap();
    let spec = SweepSpec {
        kind: SweepKind::Parameter { item: Item::Part(shifter), param: "phase" },
        start: 0.0,
        stop: std::f64::consts::TAU,
        points: 9,
    };
    let r = run(&l, 10.0, &NoDocuments, &Machine::default(), None, &spec, &no_names);
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(r.xs.len(), 9);
    assert_eq!(r.x_label, "phase (rad)");
    assert_eq!(r.series.len(), 2);
    for k in 0..9 {
        assert!((r.series[0].power[k] + r.series[1].power[k] - 1.0).abs() < 1e-9);
    }
    // Half a turn swaps the outputs.
    assert!((r.series[0].power[0] - r.series[1].power[4]).abs() < 1e-9);
    let max = r.series[0].power.iter().copied().fold(0.0, f64::max);
    let min = r.series[0].power.iter().copied().fold(1.0, f64::min);
    assert!(max > 0.99 && min < 0.01, "full contrast: {min}..{max}");
}

#[test]
fn wavelength_sweep_finds_a_ring_resonance() {
    // A laser into an add–drop ring box resonant at 1550 nm: the drop port
    // peaks there and the through port dips.
    let mut l = Layout::with_components(vec![PlacedComponent {
        kind: OpticComponentKind::Laser {
            amplitude: 1.0,
            phase: 0.0,
            wavelength: 1550.0,
            dir: Vec2::new(1.0, 0.0),
            source: 0,
            color: None,
        },
        pos: cell_center(0, 0),
    }]);
    l.add_instance(
        BlockRef::Builtin(Builtin::AddDropRing { coupling: 0.05, length_um: 20.0 * 1.55 / 2.4, n_eff: 2.4, loss_db_per_cm: 0.0, n_g: 4.2, reference_nm: 1550.0 }),
        (2, 0),
    );
    let spec = SweepSpec { kind: SweepKind::Wavelength { laser: 0 }, start: 1545.0, stop: 1555.0, points: 101 };
    let r = run(&l, 10.0, &NoDocuments, &Machine::default(), None, &spec, &no_names);
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(r.x_label, "Wavelength (nm)");
    let through = &r.series.iter().find(|s| s.name.ends_with("out1")).unwrap().power;
    let drop = &r.series.iter().find(|s| s.name.ends_with("out2")).unwrap().power;
    let peak = (0..101).max_by(|&a, &b| drop[a].total_cmp(&drop[b])).unwrap();
    assert!((r.xs[peak] - 1550.0).abs() < 0.15, "drop peaks at {}", r.xs[peak]);
    assert!(drop[peak] > 0.99 && through[peak] < 0.01);
    assert!(through[0] > 0.9, "off resonance the light passes");
    // Readouts: the drop port's peak is lossless, and the through port's
    // notch is deep.
    assert!(r.reference.iter().all(|&p| p == 1.0), "the laser's 1 mW");
    let readouts = r.readouts();
    let drop_r = readouts.iter().find(|x| x.name.ends_with("out2")).unwrap();
    assert!(drop_r.insertion_loss_db.unwrap() < 0.05 && (drop_r.peak_at - 1550.0).abs() < 0.15);
    let through_r = readouts.iter().find(|x| x.name.ends_with("out1")).unwrap();
    assert!(through_r.extinction_db > 20.0, "{}", through_r.extinction_db);
    let rel = r.relative_db(r.series.iter().find(|s| s.name.ends_with("out1")).unwrap());
    assert!(rel[0] > -0.5 && rel[50] < -20.0, "the notch at 1550 nm, relative to the laser");
    for k in 0..101 {
        assert!((through[k] + drop[k] - 1.0).abs() < 1e-9, "lossless");
    }
}

#[test]
fn sweeps_through_components_and_electronics() {
    // Tuning the WDM link's first laser: a design with components, rings
    // and electronics sweeps like any other.
    let mut p = Project::new();
    let id = examples::wdm_link(&mut p);
    let layout = p.doc(id).unwrap().layout().clone();
    let laser = layout.components.iter().position(|c| matches!(c.kind, OpticComponentKind::Laser { .. })).unwrap();
    let spec = SweepSpec { kind: SweepKind::Wavelength { laser }, start: 1525.0, stop: 1535.0, points: 21 };
    let r = run(&layout, 10.0, &p, &Machine::default(), Some(id), &spec, &no_names);
    assert!(r.error.is_none(), "{:?}", r.error);
    // Every ring is its own curve (8 rings × 2 outputs, plus 4 detectors).
    assert_eq!(r.series.len(), 20, "{:?}", r.series.iter().map(|s| &s.name).collect::<Vec<_>>());
    assert!(r.series.iter().all(|s| s.power.len() == 21));
    assert_eq!(r.series.iter().filter(|s| s.from_box).count(), 16);
    // Only near 1530 nm does the first channel's light reach its detector.
    let first = &r.series[0].power;
    let best = (0..21).max_by(|&a, &b| first[a].total_cmp(&first[b])).unwrap();
    assert!((r.xs[best] - 1530.0).abs() < 0.6, "{}", r.xs[best]);
    assert!(first[0] < 0.05 && first[20] < 0.05);
}

#[test]
fn parameters_get_set_and_csv() {
    let mut l = Layout::with_components(vec![PlacedComponent {
        kind: OpticComponentKind::BeamSplitter { reflectivity: 0.5, transparency: 0.5, normal: Vec2::new(1.0, 0.0) },
        pos: cell_center(0, 0),
    }]);
    let mzi = l.add_instance(BlockRef::Builtin(Builtin::Mzi { theta: 1.0, phi: 0.0 }), (3, 0));
    assert_eq!(params(&l, Item::Part(0)).len(), 1);
    assert_eq!(params(&l, Item::Box(mzi)).iter().map(|p| p.name).collect::<Vec<_>>(), vec!["θ", "φ"]);
    assert!(set(&mut l, Item::Part(0), "reflectivity", 0.3));
    assert!(matches!(l.components[0].kind, OpticComponentKind::BeamSplitter { transparency, .. } if (transparency - 0.7).abs() < 1e-6));
    assert_eq!(get(&l, Item::Box(mzi), "θ"), Some(1.0));
    assert!(!set(&mut l, Item::Box(mzi), "nope", 1.0));
    assert_eq!(get(&l, Item::Part(9), "phase"), None);
    assert!(params(&l, Item::Part(9)).is_empty());

    let r = SweepResult {
        x_label: "phase (rad)".into(),
        xs: vec![0.0, 1.0],
        series: vec![Series { name: "Det \"a\"".into(), power: vec![0.25, 0.5], from_box: false }],
        reference: vec![1.0, 0.0],
        error: None,
    };
    assert_eq!(r.to_csv(), "\"phase (rad)\",\"Det \"\"a\"\" power (mW)\"\n0,0.25\n1,0.5\n");
    let ro = &r.readouts()[0];
    assert_eq!((ro.peak, ro.peak_at, ro.min, ro.min_at), (0.5, 1.0, 0.25, 0.0));
    assert_eq!(ro.insertion_loss_db, None, "no input at the peak");
    assert!((ro.extinction_db - 3.0103).abs() < 1e-3);
    assert_eq!(r.relative_db(&r.series[0])[1], DB_FLOOR);

    // A laser's power sweeps in mW: amplitude² underneath.
    let mut lit = Layout::with_components(vec![PlacedComponent {
        kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
        pos: cell_center(0, 0),
    }]);
    assert_eq!(params(&lit, Item::Part(0))[0].name, "power");
    assert!(set(&mut lit, Item::Part(0), "power", 4.0));
    assert!(matches!(lit.components[0].kind, OpticComponentKind::Laser { amplitude, .. } if amplitude == 2.0));
    assert_eq!(get(&lit, Item::Part(0), "power"), Some(4.0));
    assert_eq!(source_power(&lit), 4.0);
    assert_eq!(values(0.0, 1.0, 3), vec![0.0, 0.5, 1.0]);
    assert_eq!(db(1.0), 0.0);
    assert_eq!(db(0.0), -100.0);

    // A sweep of something that isn't there says so.
    let bad = SweepSpec { kind: SweepKind::Wavelength { laser: 7 }, start: 1.0, stop: 2.0, points: 2 };
    assert!(run(&l, 10.0, &NoDocuments, &Machine::default(), None, &bad, &no_names).error.is_some());
}

#[test]
fn every_parameter_of_every_block_sets_and_reads_back() {
    // Each sweepable parameter of each built-in block (and part) takes a
    // value in its range and reports it back.
    let mut l = Layout::default();
    let ids: Vec<InstanceId> = Builtin::palette().into_iter().map(|b| l.add_instance(BlockRef::Builtin(b), (0, 0))).collect();
    l.components = OpticComponentKind::palette().iter().map(|&kind| PlacedComponent { kind, pos: cell_center(0, 0) }).collect();
    let items: Vec<Item> = ids.into_iter().map(Item::Box).chain((0..l.components.len()).map(Item::Part)).collect();
    let mut checked = 0;
    for item in items {
        for p in params(&l, item) {
            let v = p.min + (p.max - p.min) * 0.3;
            assert!(set(&mut l, item, p.name, v), "{item:?} {}", p.name);
            let back = get(&l, item, p.name).unwrap();
            assert!((back - v).abs() < 1e-4 * v.abs().max(1.0), "{item:?} {}: set {v}, got {back}", p.name);
            checked += 1;
        }
    }
    assert!(checked >= 20, "only {checked} parameters");
}

#[test]
fn an_editor_ring_with_a_group_index_has_real_resonance_spacing() {
    // A laser wired to a default add–drop ring (silicon: n_eff 2.4, n_g 4.2
    // at 1550 nm), swept from 1500 to 1600 nm: drop-port peaks sit
    // λ²/(n_g·L) apart, and one is still exactly at 1550 nm.
    let mut l = Layout::with_components(vec![PlacedComponent {
        kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
        pos: cell_center(0, 0),
    }]);
    let ring = Builtin::palette().into_iter().find(|b| matches!(b, Builtin::AddDropRing { .. })).unwrap();
    let Builtin::AddDropRing { length_um, n_g, .. } = ring else { unreachable!() };
    assert_eq!(n_g, 4.2);
    let id = l.add_instance(BlockRef::Builtin(ring), (3, 0));
    l.connect_feed(0, crate::panels::optics::model::PortRef { instance: id, port: 0 });
    let spec = SweepSpec { kind: SweepKind::Wavelength { laser: 0 }, start: 1500.0, stop: 1600.0, points: 8001 };
    let r = run(&l, 10.0, &NoDocuments, &Machine::default(), None, &spec, &no_names);
    assert!(r.error.is_none(), "{:?}", r.error);
    let drop = &r.series.iter().find(|s| s.name.ends_with("out2")).unwrap().power;
    let peaks: Vec<f64> = (1..drop.len() - 1).filter(|&k| drop[k] > drop[k - 1] && drop[k] >= drop[k + 1] && drop[k] > 0.5).map(|k| r.xs[k]).collect();
    assert!(peaks.iter().any(|p| (p - 1550.0).abs() < 0.01), "{peaks:?}");
    // With n_eff(λ) linear in λ, neighbouring resonances satisfy exactly
    // 1/λ₁ − 1/λ₂ = 1/(n_g·L), i.e. a spacing of λ₁λ₂/(n_g·L) ≈ λ²/(n_g·L).
    let length_nm = length_um * 1000.0;
    for w in peaks.windows(2) {
        let inverse = 1.0 / w[0] - 1.0 / w[1];
        assert!((inverse * 4.2 * length_nm - 1.0).abs() < 1e-3, "peaks {w:?}: 1/λ₁ − 1/λ₂ = {inverse}, want {}", 1.0 / (4.2 * length_nm));
    }
    assert!(peaks.len() >= 2, "{peaks:?}");
    // Without dispersion (n_g = n_eff) they'd be 1.75× further apart.
    let fsr_ng = 1550.0 * 1550.0 / (4.2 * length_nm);
    let fsr_neff = 1550.0 * 1550.0 / (2.4 * length_nm);
    assert!(peaks.windows(2).all(|w| (w[1] - w[0] - fsr_ng).abs() < (w[1] - w[0] - fsr_neff).abs()));
}
