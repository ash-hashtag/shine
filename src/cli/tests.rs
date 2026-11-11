use super::*;
use crate::panels::optics::examples;
use crate::panels::optics::model::{BlockRef, Builtin};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// An example saved to a file in a temporary folder.
fn saved(name: &str, build: fn(&mut Project) -> DocId) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("shine-cli-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.shine"));
    let mut p = Project::new();
    build(&mut p);
    std::fs::write(&path, p.to_bytes()).unwrap();
    let text = path.to_string_lossy().into_owned();
    (dir, text)
}

#[test]
fn arguments_are_parsed_with_helpful_errors() {
    let a = parse(&args(&["p.shine", "--wavelength", "1520:1570:11", "--laser=in", "--csv", "--out", "o.csv", "--log-level", "debug"])).unwrap();
    assert_eq!(a.project, "p.shine");
    assert_eq!(a.wavelength, Some((1520.0, 1570.0, 11)));
    assert_eq!(a.laser.as_deref(), Some("in"));
    assert!(a.csv && a.out.as_deref() == Some("o.csv"));
    let s = parse(&args(&["p.shine", "--sweep", "Add–drop ring.coupling=0.01:0.3"])).unwrap();
    assert_eq!(s.sweep, Some(("Add–drop ring".into(), "coupling".into(), 0.01, 0.3, 101)));
    assert!(parse(&args(&["--help"])).unwrap().help);
    for (bad, says) in [
        (vec![], "which project"),
        (vec!["p.shine", "--wavelength", "1520"], "FROM:TO"),
        (vec!["p.shine", "--wavelength", "a:b"], "isn't a number"),
        (vec!["p.shine", "--wavelength", "1:2:1"], "between 2"),
        (vec!["p.shine", "--sweep", "coupling=0:1"], "ITEM.PARAM"),
        (vec!["p.shine", "--frobnicate"], "unknown option"),
        (vec!["p.shine", "other.shine"], "unexpected"),
        (vec!["p.shine", "--doc"], "needs a value"),
        (vec!["p.shine", "--wavelength", "1:2", "--sweep", "x.y=0:1"], "either"),
    ] {
        let e = parse(&args(&bad)).unwrap_err();
        assert!(e.contains(says), "{bad:?}: {e}");
    }
}

#[test]
fn readings_of_the_wdm_link() {
    let (dir, path) = saved("wdm", examples::wdm_link);
    let text = simulate(&parse(&args(&[&path])).unwrap()).unwrap();
    assert!(text.contains("4-channel WDM optical link"), "{text}");
    assert!(text.contains("Detector 26,5") && text.contains(" mW"), "{text}");
    assert!(text.lines().any(|l| l.contains("Display") && l.trim_end().ends_with("11")), "the display decodes 11: {text}");
    // As CSV.
    let csv = simulate(&parse(&args(&[&path, "--csv"])).unwrap()).unwrap();
    assert!(csv.starts_with("\"reading\",\"value\",\"unit\"\n"));
    assert!(csv.lines().any(|l| l.starts_with("\"Display 26,9\",11,")), "{csv}");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn sweeps_come_out_as_csv() {
    let (dir, path) = saved("wdm-sweep", examples::wdm_link);
    let csv = simulate(&parse(&args(&[&path, "--wavelength", "1525:1535:11"])).unwrap()).unwrap();
    let mut lines = csv.lines();
    let header = lines.next().unwrap();
    assert!(header.starts_with("\"Wavelength (nm)\"") && header.contains("Detector 26,5 power (mW)"), "{header}");
    assert!(!header.contains("out2"), "box outputs only when asked");
    assert_eq!(lines.count(), 11);
    let with_boxes = simulate(&parse(&args(&[&path, "--wavelength", "1525:1535:3", "--box-outputs"])).unwrap()).unwrap();
    assert!(with_boxes.lines().next().unwrap().contains("out2"));
    // A parameter: the first ring's coupling, named by its cell.
    let p = Project::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
    let ring = p.documents[0].layout().instances.iter().find(|i| matches!(i.block, BlockRef::Builtin(Builtin::AddDropRing { .. }))).unwrap().cell;
    let spec = format!("{},{}.coupling=0.01:0.2:5", ring.0, ring.1);
    let csv = simulate(&parse(&args(&[&path, "--sweep", &spec])).unwrap()).unwrap();
    assert!(csv.starts_with("\"coupling\""), "{csv}");
    assert_eq!(csv.lines().count(), 6);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn the_adder_adds_after_the_clock() {
    let (dir, path) = saved("acc", examples::accumulator);
    let none = simulate(&parse(&args(&[&path])).unwrap()).unwrap();
    let three = simulate(&parse(&args(&[&path, "--clock", "3"])).unwrap()).unwrap();
    assert!(none.contains("Register") && three.contains("Register"), "{three}");
    assert_ne!(none, three, "the clock moves the registers");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn mistakes_are_explained_with_exit_codes() {
    let (dir, path) = saved("errors", examples::wdm_link);
    let run = |list: &[&str]| simulate(&parse(&args(list)).unwrap());
    let usage = |r: Result<String, Failure>, says: &str| match r {
        Err(Failure::Usage(m)) => assert!(m.contains(says), "{m}"),
        other => panic!("expected a usage error about {says}, got {other:?}"),
    };
    usage(run(&["/nonexistent/x.shine"]), "can't read");
    usage(run(&[&path, "--doc", "Nope"]), "no document");
    usage(run(&[&path, "--sweep", "Nope.x=0:1"]), "this design has");
    usage(run(&[&path, "--sweep", "Add–drop ring.coupling=0:1"]), "name one by its cell: ");
    usage(run(&[&path, "--sweep", "999,999.x=0:1"]), "nothing at cell");
    let ring = Project::from_bytes(&std::fs::read(&path).unwrap()).unwrap().documents[0]
        .layout()
        .instances
        .iter()
        .find(|i| matches!(i.block, BlockRef::Builtin(Builtin::AddDropRing { .. })))
        .unwrap()
        .cell;
    usage(run(&[&path, "--sweep", &format!("{},{}.nope=0:1", ring.0, ring.1)]), "it has: coupling");
    assert_eq!(Failure::Usage(String::new()).exit_code(), 2);
    assert_eq!(Failure::Simulation(String::new()).exit_code(), 1);
    assert!(simulate(&parse(&args(&["--help"])).unwrap()).unwrap().contains("Usage: shine simulate"));
    assert_eq!(main(&args(&["--bogus"])), 2);
    let out = dir.join("readings.csv");
    assert_eq!(main(&args(&[&path, "--csv", "--out", &out.to_string_lossy()])), 0);
    assert!(std::fs::read_to_string(&out).unwrap().starts_with("\"reading\""));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_components_exported_parameter_sweeps_from_the_command_line() {
    fn mz_on_a_laser(p: &mut Project) -> DocId {
        use crate::panels::optics::model::{PlacedComponent, PortRef, cell_center};
        let mz = examples::import_component(p, 1).unwrap();
        let mut l = Layout::with_components(vec![PlacedComponent {
            kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 650.0, dir: egui::Vec2::new(1.0, 0.0), source: 0, color: None },
            pos: cell_center(0, 0),
        }]);
        let id = l.add_instance(BlockRef::Component(mz), (3, 0));
        l.connect_feed(0, PortRef { instance: id, port: 0 });
        // Put the top layout first, so it's the one simulated.
        let top = p.add_layout("Switch", l, 10.0);
        p.documents.rotate_right(1);
        top
    }
    let (dir, path) = saved("mz", mz_on_a_laser);
    let csv = simulate(&parse(&args(&[&path, "--sweep", "Mach–Zehnder interferometer.phase=0:3.14159265:3", "--box-outputs"])).unwrap()).unwrap();
    assert!(csv.starts_with("\"phase (rad)\""), "{csv}");
    assert_eq!(csv.lines().count(), 4);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn export_writes_touchstone_and_gdsfactory_files() {
    use super::export::{self, Format};
    let (dir, path) = saved("export", examples::wdm_link);
    // The WDM link has no light pins: S-parameters need them; the layout doesn't.
    let args = |format, out: &str| export::Args { project: path.clone(), format, out: Some(dir.join(out).to_string_lossy().into_owned()), ..Default::default() };
    let err = export::export(&args(Some(Format::Touchstone), "wdm.s2p")).unwrap_err();
    assert!(matches!(&err, Failure::Simulation(m) if m.contains("Input and Output")), "{err:?}");
    let w = export::export(&args(None, "wdm.pic.yml")).expect("the format from the extension");
    let yaml = std::fs::read_to_string(&w.path).unwrap();
    assert!(yaml.contains("component: ring_double") && yaml.contains("routes:"), "{yaml}");
    assert!(w.notes.iter().any(|n| n.contains("free-space")), "{:?}", w.notes);
    std::fs::remove_dir_all(&dir).ok();

    let parsed = export::parse(&["p.shine".into(), "--format=gdsfactory".into(), "--spacing".into(), "25".into()]).unwrap();
    assert_eq!((parsed.format, parsed.spacing), (Some(Format::Gdsfactory), Some(25.0)));
    assert!(export::parse(&["p.shine".into(), "--format".into(), "gerber".into()]).is_err());
}

#[test]
fn a_wavelength_sweep_reports_the_rings_resonances() {
    let (dir, path) = saved("readouts", examples::wdm_link);
    let args = parse(&[path, "--wavelength".into(), "1520:1570:2001".into(), "--readouts".into()]).unwrap();
    let csv = simulate(&args).unwrap();
    let header = csv.lines().next().unwrap();
    assert!(header.contains("\"FSR (nm)\"") && header.contains("\"Q\""), "{header}");
    // A detector behind a drop port: peaks, with a width and a Q.
    let row = csv.lines().find(|l| l.contains("peaks") && !l.ends_with(",,")).unwrap_or_else(|| panic!("{csv}"));
    let q: f64 = row.rsplit(',').next().unwrap().parse().unwrap();
    assert!(q > 100.0, "a ring's Q: {row}");
    std::fs::remove_dir_all(&dir).ok();
}
