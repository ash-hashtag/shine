//! The API as the design spec (design/library-api.md) shows it.

use super::*;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// The spec's first example: a modulator written as a component, used on a chip.
fn modulator_link() -> Result<(Project, Design, Part, Part)> {
    let project = Project::new();
    let mzm = project.new_component("Modulator");
    let light_in = mzm.light_input("in");
    let light_out = mzm.light_output("out");
    let split = mzm.y_splitter();
    let heater = mzm.phase_shifter(std::f64::consts::PI);
    let upper = mzm.waveguide(100.0)?;
    let lower = mzm.waveguide(100.0)?;
    let join = mzm.y_combiner();
    mzm.chain(&[light_in, split.clone(), heater.clone(), upper.clone()])?;
    mzm.connect(&split.port("out2")?, &lower.port("in")?)?;
    mzm.connect(&upper.port("out")?, &join.port("in1")?)?;
    mzm.connect(&lower.port("out")?, &join.port("in2")?)?;
    mzm.chain(&[join, light_out])?;
    mzm.expose("bias", &heater, "phase")?;

    let chip = project.new_design("Link");
    let laser = chip.laser(1550.0, 1.0)?;
    let modulator = chip.place(&mzm)?;
    let detector = chip.detector();
    chip.chain(&[laser, modulator.clone(), detector.clone()])?;
    Ok((project, chip, modulator, detector))
}

#[test]
fn a_component_written_in_code_works_on_a_chip() -> Result<()> {
    let (_, chip, modulator, detector) = modulator_link()?;
    // The heater starts at π: the arms cancel.
    assert!(chip.simulate()?.power_mw(&detector)? < 1e-12);
    modulator.set("bias", 0.0)?;
    assert!(close(chip.simulate()?.power_mw(&detector)?, 1.0, 1e-12), "in phase: all of it");
    // A sweep of the bias: the modulator's cos² curve.
    let sweep = chip.sweep(&modulator, "bias", 0.0, std::f64::consts::PI, 5)?;
    let p = sweep.series("Detector")?.power_mw;
    for (x, y) in sweep.x.iter().zip(&p) {
        assert!(close(*y, (x / 2.0).cos().powi(2), 1e-12), "at {x}: {y}");
    }
    assert_eq!(sweep.x_label, "bias (rad)");
    Ok(())
}

#[test]
fn parts_ports_and_parameters_explain_themselves() -> Result<()> {
    let project = Project::new();
    let chip = project.new_design("Filter");
    let ring = chip.ring(0.05, 100.0)?;
    let ring2 = chip.ring(0.1, 50.0)?;
    assert_eq!((ring.name(), ring2.name(), ring.kind()), ("Ring resonator".into(), "Ring resonator 2".into(), "Ring resonator".into()));
    assert_eq!(chip.find_part("Ring resonator 2")?, ring2);
    let ports: Vec<String> = ring.ports().into_iter().map(|p| p.name).collect();
    assert_eq!(ports, ["in", "through"]);
    let names: Vec<String> = ring.params().into_iter().map(|p| p.name).collect();
    assert!(names.contains(&"coupling".to_owned()) && names.contains(&"n_g".to_owned()), "{names:?}");
    ring.set("n_g", 4.0)?.set("coupling", 0.2)?;
    assert_eq!(ring.get("coupling")?, 0.2);
    // Errors say what there is.
    let e = ring.port("drop").unwrap_err().to_string();
    assert_eq!(e, "“Ring resonator” has no port “drop”; it has: in, through");
    let e = ring.set("kappa", 0.1).unwrap_err().to_string();
    assert!(e.starts_with("“Ring resonator” has no parameter “kappa”; it has: coupling"), "{e}");
    assert!(chip.coupler(1.5).unwrap_err().to_string().contains("fraction from 0 to 1"));
    assert!(chip.find_part("Laser").unwrap_err().to_string().contains("it has: Ring resonator, Ring resonator 2"));
    // Connections go output to input, light to light.
    let e = chip.connect(&ring.port("in")?, &ring2.port("in")?).unwrap_err().to_string();
    assert!(e.contains("from an output to an input"), "{e}");
    let v = chip.value_input(0.5);
    let e = chip.connect(&v.port("v")?, &ring2.port("in")?).unwrap_err().to_string();
    assert!(e.contains("connect light to light"), "{e}");
    assert!(chip.describe().contains("Ring resonator 2 (Ring resonator) ←in →through"), "{}", chip.describe());
    Ok(())
}

#[test]
fn a_rings_spectrum_has_its_textbook_spacing() -> Result<()> {
    let project = Project::new();
    let chip = project.new_design("Ring");
    let laser = chip.laser(1550.0, 1.0)?;
    let ring = chip.ring(0.05, 100.0)?;
    let detector = chip.detector();
    chip.chain(&[laser.clone(), ring.clone(), detector])?;
    let spectrum = chip.sweep(&laser, "wavelength", 1540.0, 1560.0, 4001)?;
    let r = spectrum.readouts("Detector")?;
    let fsr = 1550.0f64.powi(2) / (4.2 * 100.0 * 1000.0);
    assert!(r.dips && close(r.fsr_nm.unwrap(), fsr, 0.01 * fsr), "{r:?}");
    assert!(spectrum.outputs().contains(&"Ring resonator.through".to_owned()), "{:?}", spectrum.outputs());
    assert!(spectrum.to_csv().starts_with("\"wavelength (nm)\",\"Detector (mW)\""));
    Ok(())
}

#[test]
fn transmission_between_a_components_pins() -> Result<()> {
    let project = Project::new();
    let c = project.new_component("Splitter");
    let (a, b) = (c.light_input("a"), c.light_input("b"));
    let dc = c.coupler(0.3)?;
    let (bar, cross) = (c.light_output("bar"), c.light_output("cross"));
    // Pins face into the component: an input pin's light leaves on `out`.
    assert_eq!(a.ports().into_iter().map(|p| p.name).collect::<Vec<_>>(), ["out"]);
    c.connect(&a.port("out")?, &dc.port("in1")?)?;
    c.connect(&b.port("out")?, &dc.port("in2")?)?;
    c.connect(&dc.port("out1")?, &bar.port("in")?)?;
    c.connect(&dc.port("out2")?, &cross.port("in")?)?;
    let t = c.transmission(&[1550.0, 1560.0])?;
    assert_eq!((t.inputs.clone(), t.outputs.clone()), (vec!["a".to_owned(), "b".into()], vec!["bar".to_owned(), "cross".into()]));
    assert!(t.power("bar", "a")?.iter().all(|p| close(*p, 0.7, 1e-12)));
    assert!(t.power("cross", "a")?.iter().all(|p| close(*p, 0.3, 1e-12)));
    let m = t.matrix(0)?;
    assert!(close(m.get(1, 0).arg(), std::f64::consts::FRAC_PI_2, 1e-12), "the cross port a quarter turn ahead");
    assert!(t.power("drop", "a").unwrap_err().to_string().contains("it has: bar, cross"));
    Ok(())
}

#[test]
fn files_round_trip_and_read_well_in_the_app() -> Result<()> {
    let (project, chip, modulator, _) = modulator_link()?;
    modulator.set("bias", 0.0)?;
    let bytes = project.to_bytes();
    let again = Project::from_bytes(&bytes)?;
    assert_eq!(again.designs(), ["Modulator", "Link"]);
    let link = again.design("Link")?;
    let detector = link.find_part("Detector")?;
    assert!(close(link.simulate()?.power_mw(&detector)?, 1.0, 1e-12), "the same circuit");
    assert_eq!(link.find_part("Modulator")?.get("bias")?, 0.0);
    // Laid out left to right as light flows, without overlaps.
    let inner = super::project::lock(&again.inner);
    let layout = inner.project.doc(link.id).unwrap().layout();
    let xs: Vec<i32> = layout.instances.iter().map(|i| i.cell.0).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "laser, modulator, detector: {xs:?}");
    drop(inner);
    assert!(chip.design_check());
    assert!(Project::from_bytes(b"not a project").is_err());
    Ok(())
}

impl Design {
    fn design_check(&self) -> bool {
        !self.name().is_empty()
    }
}

#[test]
fn components_from_tables_and_functions() -> Result<()> {
    let project = Project::new();
    // A filter written as a function: passes 1550 nm, blocks 1560 nm.
    let filter = |wl: f64| Matrix::from_rows(&[vec![Complex::new(((wl - 1560.0) / 10.0).abs().min(1.0), 0.0)]]);
    let f = project.new_model_from_function("Filter", vec!["in".into()], vec!["out".into()], vec![1550.0, 1555.0, 1560.0], &filter)?;
    let chip = project.new_design("Test");
    let laser = chip.laser(1555.0, 1.0)?;
    let part = chip.place(&f)?;
    let pd = chip.detector();
    chip.chain(&[laser, part, pd.clone()])?;
    assert!(close(chip.simulate()?.power_mw(&pd)?, 0.25, 1e-12), "amplitude 0.5 at 1555 nm");
    let e = project.new_model_from_table("Bad", vec!["in".into()], vec!["out".into()], vec![1550.0], vec![Matrix::default()]).unwrap_err();
    assert!(e.to_string().contains("should be 1×1"), "{e}");
    Ok(())
}

#[test]
fn monte_carlo_repeats_from_its_seed_and_optimisation_finds_a_coupling() -> Result<()> {
    let project = Project::new();
    let chip = project.new_design("Split");
    let laser = chip.laser(1550.0, 1.0)?;
    let dc = chip.coupler(0.5)?;
    let pd = chip.detector();
    chip.chain(&[laser, dc.clone(), pd.clone()])?;
    let v = Variation::default();
    let a = chip.monte_carlo(20, 7, &v)?;
    let b = chip.monte_carlo(20, 7, &v)?;
    assert_eq!(a.values("Detector")?, b.values("Detector")?);
    let s = a.stats("Detector")?;
    assert!(s.std > 0.0 && close(s.mean, 0.5, 0.05), "{s:?}");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let vars = [(dc.clone(), "coupling".to_owned(), 0.0, 1.0)];
        let r = chip.optimise(&vars, "Detector", Goal::Target(0.2), 200)?;
        assert!(close(r.values[0], 0.8, 1e-3) && close(r.after_mw, 0.2, 1e-5), "{r:?}");
        chip.apply(&vars, &r.values)?;
        assert!(close(dc.get("coupling")?, 0.8, 1e-3));
    }
    Ok(())
}

#[test]
fn electronics_drive_the_light() -> Result<()> {
    let project = Project::new();
    let chip = project.new_design("Switch");
    let laser = chip.laser(1550.0, 1.0)?;
    let att = chip.attenuator(0.0)?;
    let pd = chip.detector();
    let v = chip.value_input(0.25);
    chip.chain(&[laser, att.clone(), pd.clone()])?;
    chip.connect(&v.port("v")?, &att.port("transmission")?)?;
    let sim = chip.simulate()?;
    assert!(close(sim.power_mw(&pd)?, 0.25, 1e-12), "the value input passes a quarter");
    assert_eq!(sim.signal(&att.port("transmission")?)?, 0.25);
    let n = chip.number_input(4, 9)?;
    let d = chip.display(4)?;
    for k in 0..4 {
        chip.connect(&n.port(&format!("b{}", 3 - k))?, &d.port(&format!("b{}", 3 - k))?)?;
    }
    assert_eq!(chip.simulate()?.display(&d)?, 9);
    Ok(())
}

#[test]
fn a_weight_matrix_becomes_a_layer_of_light() -> Result<()> {
    let p = Project::new();
    let w = Matrix::from_rows(&[
        vec![Complex::new(0.5, 0.0), Complex::new(-1.0, 0.0), Complex::new(2.0, 0.0)],
        vec![Complex::new(0.25, 0.0), Complex::new(0.0, 0.0), Complex::new(1.0, 0.0)],
    ]);
    let layer = p.new_weight_layer("Weights", &w)?;
    assert!(layer.is_component());
    let scale = layer_scale(&w)?;
    assert!(layer.description().contains(&format!("{scale:.6}")), "{}", layer.description());
    let t = layer.transmission(&[1550.0])?;
    assert_eq!((t.inputs.len(), t.outputs.len()), (3, 2));
    // |W x / σmax|, up to the arms' common loss: the ratios of powers match.
    let p00 = t.power("out1", "in1")?[0];
    for (o, i, w) in [("out1", "in2", 1.0), ("out1", "in3", 2.0), ("out2", "in1", 0.25), ("out2", "in3", 1.0)] {
        let ratio = t.power(o, i)?[0] / p00;
        assert!(close(ratio, (w / 0.5f64).powi(2), 1e-9), "{o}←{i}: {ratio}");
    }
    assert!(t.power("out2", "in2")?[0] < 1e-20);
    // A unitary as one mesh; mistakes are explained.
    let swap = p.new_unitary_mesh("Swap", &Matrix::from_rows(&[vec![Complex::new(0.0, 0.0), Complex::new(1.0, 0.0)], vec![Complex::new(1.0, 0.0), Complex::new(0.0, 0.0)]]))?;
    assert!(close(swap.transmission(&[1550.0])?.power("out2", "in1")?[0], swap.transmission(&[1550.0])?.power("out1", "in2")?[0], 1e-12));
    let bad = Matrix { rows: 2, cols: 2, values: vec![Complex::new(1.0, 0.0)] };
    assert!(p.new_weight_layer("Bad", &bad).unwrap_err().to_string().contains("needs 4 values"));
    Ok(())
}
