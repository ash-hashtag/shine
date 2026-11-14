//! `shine simulate`: simulate a saved project from the command line, for
//! scripts and CI. Prints every reading, or a sweep as CSV.

use std::fmt::Write as _;

use crate::panels::circuit::readings;
use crate::panels::circuit::sim::{self, Machine, Options};
use crate::panels::circuit::sweep::{self, SweepKind, SweepSpec};
use crate::panels::optics::model::{BlockRef, Builtin, InstanceId, Item, Layout, OpticComponentKind, cell_of};
use crate::panels::project::{DocId, Project};
use crate::panels::units;

pub const USAGE: &str = "\
Usage: shine simulate PROJECT.shine [options]

Simulates a saved project and prints every reading (detectors, output ports,
displays, registers), or a sweep as CSV.

Options:
  --doc NAME                      the document to simulate (default: the first one
                                  that isn't a component)
  --wavelength FROM:TO[:POINTS]   sweep a laser's wavelength in nm (default 201 points)
  --laser ITEM                    the laser to tune (default: the first)
  --sweep ITEM.PARAM=FROM:TO[:POINTS]
                                  sweep a parameter, e.g. --sweep 'Phase shifter.phase=0:6.28'
  --sweep2 ITEM.PARAM=FROM:TO[:POINTS]
                                  with --wavelength or --sweep: also vary a second
                                  parameter (default 21 points), giving a map as CSV
                                  (one row per point: x, y, each output's power)
  --monte-carlo N                 simulate N copies under fabrication variation: each
                                  sample's output powers as CSV (with --readouts: each
                                  output's nominal, mean, σ, 5 % and 95 %, min and max)
  --seed S                        Monte Carlo: the random seed (default 1)
  --vary-neff X, --vary-coupling X, --vary-phase X, --vary-loss X
                                  Monte Carlo: the variation's σ (index, relative
                                  coupling, radians, relative loss; defaults 0.002,
                                  0.03, 0.05, 0.1)
  --box-outputs                   also include the light leaving boxes
  --readouts                      with a sweep: each output's peak, minimum, insertion
                                  loss, extinction and resonances (FSR, width, Q) as CSV,
                                  instead of the curves
  --input FILE                    set the design's inputs (value and number inputs, in
                                  the order --inputs lists) from numbers in FILE, or
                                  from standard input with '-': separated by spaces,
                                  commas or lines, '#' starting a comment. Several
                                  samples' numbers one after another simulate each,
                                  a CSV row per sample
  --inputs                        list the design's inputs, in order, and stop
  --only TEXT                     only the readings whose names contain TEXT (again
                                  for more), e.g. --box-outputs --only 'Layer 2'
  --clock N                       tick the clock N times first (registers)
  --csv                           readings as CSV (sweeps are always CSV)
  --out FILE                      write to FILE instead of standard output
  -h, --help                      this help

ITEM is a part's label (e.g. a port's name), a block or part name when it's unique
(e.g. 'Add–drop ring'), or a grid cell, e.g. 24,4.

Exit status: 0 on success, 1 if the simulation failed, 2 for a usage error.";

/// What went wrong, and the exit status for it.
#[derive(Debug, PartialEq)]
pub enum Failure {
    /// Bad arguments or a missing file: exit 2.
    Usage(String),
    /// The design didn't simulate: exit 1.
    Simulation(String),
}

impl Failure {
    pub fn exit_code(&self) -> i32 {
        match self {
            Failure::Usage(_) => 2,
            Failure::Simulation(_) => 1,
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub project: String,
    pub doc: Option<String>,
    pub wavelength: Option<(f64, f64, usize)>,
    pub laser: Option<String>,
    pub sweep: Option<(String, String, f64, f64, usize)>,
    pub sweep2: Option<(String, String, f64, f64, usize)>,
    pub monte_carlo: Option<usize>,
    pub seed: Option<u64>,
    pub variation: crate::panels::circuit::monte_carlo::Variation,
    pub box_outputs: bool,
    pub readouts: bool,
    pub clock: u64,
    pub input: Option<String>,
    pub list_inputs: bool,
    pub only: Vec<String>,
    pub csv: bool,
    pub out: Option<String>,
    pub help: bool,
}

/// `FROM:TO[:POINTS]`.
pub(crate) fn range(text: &str, default_points: usize) -> Result<(f64, f64, usize), String> {
    let parts: Vec<&str> = text.split(':').collect();
    let num = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("“{s}” isn't a number (expected FROM:TO[:POINTS])"));
    match parts.as_slice() {
        [from, to] => Ok((num(from)?, num(to)?, default_points)),
        [from, to, points] => {
            let n: usize = points.trim().parse().map_err(|_| format!("“{points}” isn't a number of points"))?;
            if !(2..=100_000).contains(&n) {
                return Err("points must be between 2 and 100000".into());
            }
            Ok((num(from)?, num(to)?, n))
        }
        _ => Err(format!("“{text}”: expected FROM:TO[:POINTS]")),
    }
}

/// Arguments after `simulate`.
pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") && f != "--sweep" && f != "--sweep2" => (f, Some(v.to_owned())),
            _ => (arg.as_str(), None),
        };
        let mut value = |name: &str| inline.clone().or_else(|| it.next().cloned()).ok_or(format!("{name} needs a value"));
        match flag {
            "-h" | "--help" => a.help = true,
            "--doc" => a.doc = Some(value("--doc")?),
            "--wavelength" => a.wavelength = Some(range(&value("--wavelength")?, 201)?),
            "--laser" => a.laser = Some(value("--laser")?),
            "--sweep" => {
                let spec = value("--sweep")?;
                let (target, span) = spec.split_once('=').ok_or("--sweep needs ITEM.PARAM=FROM:TO")?;
                let (item, param) = target.rsplit_once('.').ok_or("--sweep needs ITEM.PARAM, e.g. 'Coupler.coupling'")?;
                let (from, to, points) = range(span, 101)?;
                a.sweep = Some((item.trim().to_owned(), param.trim().to_owned(), from, to, points));
            }
            "--sweep2" => {
                let spec = value("--sweep2")?;
                let (target, span) = spec.split_once('=').ok_or("--sweep2 needs ITEM.PARAM=FROM:TO")?;
                let (item, param) = target.rsplit_once('.').ok_or("--sweep2 needs ITEM.PARAM, e.g. 'Coupler.coupling'")?;
                let (from, to, points) = range(span, 21)?;
                a.sweep2 = Some((item.trim().to_owned(), param.trim().to_owned(), from, to, points));
            }
            "--monte-carlo" => {
                let n: usize = value("--monte-carlo")?.parse().map_err(|_| "--monte-carlo needs a number of samples")?;
                if !(1..=1_000_000).contains(&n) {
                    return Err("--monte-carlo takes 1 to 1000000 samples".into());
                }
                a.monte_carlo = Some(n);
            }
            "--seed" => a.seed = Some(value("--seed")?.parse().map_err(|_| "--seed needs a whole number")?),
            "--vary-neff" | "--vary-coupling" | "--vary-phase" | "--vary-loss" => {
                let name = flag.to_owned();
                let x: f64 = value(&name)?.parse().map_err(|_| format!("{name} needs a number"))?;
                if !(x >= 0.0 && x.is_finite()) {
                    return Err(format!("{name} can't be negative"));
                }
                match name.as_str() {
                    "--vary-neff" => a.variation.n_eff = x,
                    "--vary-coupling" => a.variation.coupling = x,
                    "--vary-phase" => a.variation.phase = x,
                    _ => a.variation.loss = x,
                }
            }
            "--box-outputs" => a.box_outputs = true,
            "--readouts" => a.readouts = true,
            "--clock" => a.clock = value("--clock")?.parse().map_err(|_| "--clock needs a number of ticks")?,
            "--input" => a.input = Some(value("--input")?),
            "--inputs" => a.list_inputs = true,
            "--only" => a.only.push(value("--only")?),
            "--csv" => a.csv = true,
            "--out" => a.out = Some(value("--out")?),
            // Logging options belong to the binary.
            "--log-file" | "--log-level" => {
                if inline.is_none() {
                    it.next();
                }
            }
            f if f.starts_with('-') => return Err(format!("unknown option {f}")),
            path if a.project.is_empty() => a.project = path.to_owned(),
            extra => return Err(format!("unexpected argument “{extra}”")),
        }
    }
    if a.project.is_empty() && !a.help {
        return Err("which project? (shine simulate PROJECT.shine)".into());
    }
    if a.wavelength.is_some() && a.sweep.is_some() {
        return Err("use either --wavelength or --sweep, not both".into());
    }
    if a.monte_carlo.is_some() && (a.wavelength.is_some() || a.sweep.is_some()) {
        return Err("--monte-carlo varies the design at its own settings: leave out --wavelength and --sweep".into());
    }
    if a.sweep2.is_some() && a.wavelength.is_none() && a.sweep.is_none() {
        return Err("--sweep2 adds a second axis to --wavelength or --sweep".into());
    }
    Ok(a)
}

/// The part or box `spec` names in `layout`: a label, a unique name, or a cell `x,y`.
pub fn find_item(layout: &Layout, project: &Project, spec: &str) -> Result<Item, String> {
    let catalog = project.catalog();
    if let Some((x, y)) = spec.split_once(',').and_then(|(x, y)| Some((x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?))) {
        if let Some(i) = layout.components.iter().position(|c| cell_of(c.pos) == (x, y)) {
            return Ok(Item::Part(i));
        }
        if let Some(inst) = layout.instances.iter().find(|i| i.cell == (x, y)) {
            return Ok(Item::Box(inst.id));
        }
        return Err(format!("nothing at cell {x},{y}"));
    }
    if let Some(i) = (0..layout.components.len()).find(|&i| layout.label(i) == Some(spec)) {
        return Ok(Item::Part(i));
    }
    let names: Vec<(Item, String)> = layout
        .components
        .iter()
        .enumerate()
        .map(|(i, c)| (Item::Part(i), c.kind.name().to_owned()))
        .chain(layout.instances.iter().map(|inst| (Item::Box(inst.id), catalog.display(&inst.block).name)))
        .collect();
    let matching: Vec<&(Item, String)> = names.iter().filter(|(_, n)| n.eq_ignore_ascii_case(spec)).collect();
    match matching.as_slice() {
        [(item, _)] => Ok(*item),
        [] => {
            let mut known: Vec<&str> = names.iter().map(|(_, n)| n.as_str()).collect();
            known.sort_unstable();
            known.dedup();
            Err(format!("no part or block called “{spec}”; this design has: {}", known.join(", ")))
        }
        many => {
            let cells: Vec<String> = many
                .iter()
                .map(|(item, _)| {
                    let (x, y) = match *item {
                        Item::Part(i) => cell_of(layout.components[i].pos),
                        Item::Box(id) => layout.instance(id).map_or((0, 0), |i| i.cell),
                    };
                    format!("{x},{y}")
                })
                .collect();
            Err(format!("{} things are called “{spec}”; name one by its cell: {}", many.len(), cells.join("  ")))
        }
    }
}

/// The design's inputs, in the order `--input` sets them: its value inputs
/// and number inputs, as the layout holds them.
pub fn design_inputs(layout: &Layout) -> Vec<InstanceId> {
    layout
        .instances
        .iter()
        .filter(|i| matches!(i.block, BlockRef::Builtin(Builtin::Value { .. } | Builtin::NumberInput { .. })))
        .map(|i| i.id)
        .collect()
}

/// Numbers separated by spaces, commas, semicolons or lines; `#` to the end
/// of a line is a comment.
pub fn read_numbers(text: &str) -> Result<Vec<f64>, String> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .flat_map(|l| l.split(|c: char| c.is_whitespace() || c == ',' || c == ';'))
        .filter(|t| !t.is_empty())
        .enumerate()
        .map(|(k, t)| t.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or(format!("input {}: “{t}” isn't a number", k + 1)))
        .collect()
}

/// Set input `id` to `v`: a value input to `v`, a number input to `v`
/// rounded into its bits' range.
fn set_input(layout: &mut Layout, id: InstanceId, v: f64) {
    if let Some(inst) = layout.instances.iter_mut().find(|i| i.id == id) {
        match &mut inst.block {
            BlockRef::Builtin(Builtin::Value { value, .. }) => *value = v,
            BlockRef::Builtin(Builtin::NumberInput { bits, value }) => {
                let max = if *bits >= 64 { u64::MAX as f64 } else { ((1u64 << *bits) - 1) as f64 };
                *value = v.round().clamp(0.0, max) as u64;
            }
            _ => {}
        }
    }
}

/// Whether a reading is wanted: light leaving boxes only with
/// `--box-outputs`, and with `--only`, names containing one of its texts.
fn wanted(args: &Args, r: &readings::Reading) -> bool {
    (!r.from_box || args.box_outputs) && (args.only.is_empty() || args.only.iter().any(|t| r.name.contains(t.as_str())))
}

/// `--inputs`: each input with its place in the stream, name and cell.
fn list_inputs(layout: &Layout) -> String {
    let inputs = design_inputs(layout);
    let mut out = format!("{} inputs, in the order --input sets them:\n", inputs.len());
    for (k, id) in inputs.iter().enumerate() {
        let inst = layout.instance(*id).expect("listed");
        let (kind, name, now) = match &inst.block {
            BlockRef::Builtin(Builtin::Value { name, value }) => ("value", name.as_str(), value.to_string()),
            BlockRef::Builtin(Builtin::NumberInput { bits, value }) => ("number", "", format!("{value} ({bits} bits)")),
            _ => continue,
        };
        let name = if name.is_empty() { String::new() } else { format!(" “{name}”") };
        let _ = writeln!(out, "  {:>4}  {kind}{name} at {},{}  now {now}", k + 1, inst.cell.0, inst.cell.1);
    }
    out
}

/// Run `shine simulate` with `args` (after `simulate`); returns the text to print.
pub fn simulate(args: &Args) -> Result<String, Failure> {
    if args.help {
        return Ok(format!("{USAGE}\n"));
    }
    let bytes = std::fs::read(&args.project).map_err(|e| Failure::Usage(format!("can't read {}: {e}", args.project)))?;
    let mut project = Project::from_bytes(&bytes).map_err(|e| Failure::Usage(format!("{}: {e}", args.project)))?;
    let id = choose_doc(&project, args.doc.as_deref()).map_err(Failure::Usage)?;
    if args.list_inputs {
        return Ok(list_inputs(project.doc(id).expect("chosen").layout()));
    }
    // The inputs' values, a sample after another.
    if let Some(source) = &args.input {
        let text = if source == "-" {
            std::io::read_to_string(std::io::stdin()).map_err(|e| Failure::Usage(format!("can't read standard input: {e}")))?
        } else {
            std::fs::read_to_string(source).map_err(|e| Failure::Usage(format!("can't read {source}: {e}")))?
        };
        let numbers = read_numbers(&text).map_err(Failure::Usage)?;
        let n = design_inputs(project.doc(id).expect("chosen").layout()).len();
        if n == 0 {
            return Err(Failure::Usage("this design has no value or number inputs to set".into()));
        }
        if numbers.is_empty() || numbers.len() % n != 0 {
            return Err(Failure::Usage(format!(
                "the design has {n} inputs, so --input needs {n} numbers per sample; it got {}",
                numbers.len()
            )));
        }
        let samples: Vec<Vec<f64>> = numbers.chunks(n).map(<[f64]>::to_vec).collect();
        if samples.len() > 1 && (args.monte_carlo.is_some() || args.wavelength.is_some() || args.sweep.is_some() || args.clock > 0) {
            return Err(Failure::Usage("several samples simulate one after another: leave out sweeps, --monte-carlo and --clock".into()));
        }
        if samples.len() > 1 {
            return simulate_samples(&mut project, id, args, &samples);
        }
        let inputs = design_inputs(project.doc(id).expect("chosen").layout());
        let layout = project.doc_mut(id).expect("chosen").layout_mut();
        for (&input, &v) in inputs.iter().zip(&samples[0]) {
            set_input(layout, input, v);
        }
    }
    let project = project;
    let doc = project.doc(id).expect("chosen");
    let layout = doc.layout();
    let cell_um = doc.panel().cell_um;
    let catalog = project.catalog();
    let names = |id| layout.instance(id).map_or_else(|| "Box".into(), |i| catalog.display(&i.block).name);

    let mut machine = Machine::default();
    for _ in 0..args.clock {
        let o = sim::simulate(layout, cell_um, &project, &machine, Some(id), &Options::default());
        sim::clock(layout, &project, &mut machine, &o);
    }

    let spec = if let Some((from, to, points)) = args.wavelength {
        let laser = match &args.laser {
            Some(spec) => match find_item(layout, &project, spec).map_err(Failure::Usage)? {
                Item::Part(i) if matches!(layout.components[i].kind, OpticComponentKind::Laser { .. }) => i,
                _ => return Err(Failure::Usage(format!("“{spec}” isn't a laser"))),
            },
            None => layout
                .components
                .iter()
                .position(|c| matches!(c.kind, OpticComponentKind::Laser { .. }))
                .ok_or(Failure::Usage("this design has no laser to tune".into()))?,
        };
        Some(SweepSpec { kind: SweepKind::Wavelength { laser }, start: from, stop: to, points })
    } else if let Some((item, param, from, to, points)) = &args.sweep {
        let item = find_item(layout, &project, item).map_err(Failure::Usage)?;
        let available = sweep::params_in(layout, item, &project);
        let Some(p) = available.iter().find(|p| &p.name == param) else {
            let names: Vec<&str> = available.iter().map(|p| p.name.as_str()).collect();
            return Err(Failure::Usage(if names.is_empty() {
                "that has no parameters to sweep".into()
            } else {
                format!("no parameter “{param}”; it has: {}", names.join(", "))
            }));
        };
        Some(SweepSpec { kind: SweepKind::Parameter { item, param: p.name.clone() }, start: *from, stop: *to, points: *points })
    } else {
        None
    };

    if let Some(samples) = args.monte_carlo {
        let mut mc = crate::panels::circuit::monte_carlo::run(layout, cell_um, &project, &machine, Some(id), &args.variation, samples, args.seed.unwrap_or(1), &names);
        if let Some(e) = mc.error.take() {
            return Err(Failure::Simulation(e));
        }
        let only_boxes = mc.series.iter().all(|s| s.from_box);
        if !args.box_outputs && !only_boxes {
            mc.series.retain(|s| !s.from_box);
        }
        if !args.readouts {
            return Ok(mc.to_csv());
        }
        let mut out = "\"output\",\"nominal (mW)\",\"mean (mW)\",\"σ (mW)\",\"5 % (mW)\",\"95 % (mW)\",\"min (mW)\",\"max (mW)\"\n".to_owned();
        for s in &mc.series {
            let st = s.stats();
            let _ = writeln!(out, "\"{}\",{},{},{},{},{},{},{}", s.name.replace('"', "\"\""), s.nominal, st.mean, st.std, st.p5, st.p95, st.min, st.max);
        }
        return Ok(out);
    }

    if let (Some(spec), Some((item, param, from, to, points))) = (&spec, &args.sweep2) {
        let item = find_item(layout, &project, item).map_err(Failure::Usage)?;
        let available = sweep::params_in(layout, item, &project);
        if !available.iter().any(|p| &p.name == param) {
            let names: Vec<&str> = available.iter().map(|p| p.name.as_str()).collect();
            return Err(Failure::Usage(format!("no parameter “{param}” for --sweep2; it has: {}", names.join(", "))));
        }
        let axis = sweep::Axis { item, param: param.clone(), start: *from, stop: *to, points: *points };
        let mut map = sweep::run_map(layout, cell_um, &project, &machine, Some(id), spec, &axis, &names);
        if let Some(e) = map.error.take() {
            return Err(Failure::Simulation(e));
        }
        let only_boxes = map.series.iter().all(|s| s.from_box);
        if !args.box_outputs && !only_boxes {
            map.series.retain(|s| !s.from_box);
        }
        return Ok(map.to_csv());
    }

    if let Some(spec) = spec {
        let mut result = sweep::run(layout, cell_um, &project, &machine, Some(id), &spec, &names);
        if let Some(e) = result.error.take() {
            return Err(Failure::Simulation(e));
        }
        let only_boxes = result.series.iter().all(|s| s.from_box);
        if !args.box_outputs && !only_boxes {
            result.series.retain(|s| !s.from_box);
        }
        return Ok(if args.readouts { result.readouts_csv() } else { result.to_csv() });
    }

    let o = sim::simulate(layout, cell_um, &project, &machine, Some(id), &Options::default());
    if let Some(e) = &o.error {
        return Err(Failure::Simulation(e.clone()));
    }
    let rows: Vec<readings::Reading> =
        readings::readings(layout, &o, &names).into_iter().filter(|r| wanted(args, r)).collect();
    let mut out = String::new();
    if args.csv {
        out = readings::to_csv(&rows);
    } else {
        let width = rows.iter().map(|r| r.name.chars().count()).max().unwrap_or(0);
        let _ = writeln!(out, "{} · {} · settled in {} rounds", doc.name, args.project, o.iterations);
        for r in &rows {
            let _ = writeln!(out, "  {:<width$}  {}", r.name, r.shown(units::PowerUnit::MilliWatt));
        }
        if rows.is_empty() {
            out += "  (no detectors, output ports, displays or registers)\n";
        }
    }
    Ok(out)
}

/// Several samples of the inputs, one simulation each: a CSV row per sample,
/// the sample's number then every reading.
fn simulate_samples(project: &mut Project, id: DocId, args: &Args, samples: &[Vec<f64>]) -> Result<String, Failure> {
    let inputs = design_inputs(project.doc(id).expect("chosen").layout());
    // What readings are called doesn't change with the inputs: worked out
    // once (it reads every document, a big mesh's included).
    let catalog = project.catalog();
    let mut out = String::new();
    for (k, sample) in samples.iter().enumerate() {
        let layout = project.doc_mut(id).expect("chosen").layout_mut();
        for (&input, &v) in inputs.iter().zip(sample) {
            set_input(layout, input, v);
        }
        let doc = project.doc(id).expect("chosen");
        let (layout, cell_um) = (doc.layout(), doc.panel().cell_um);
        let o = sim::simulate(layout, cell_um, &*project, &Machine::default(), Some(id), &Options::default());
        if let Some(e) = &o.error {
            return Err(Failure::Simulation(format!("sample {}: {e}", k + 1)));
        }
        let names = |id| layout.instance(id).map_or_else(|| "Box".into(), |i| catalog.display(&i.block).name);
        let rows: Vec<readings::Reading> =
            readings::readings(layout, &o, &names).into_iter().filter(|r| wanted(args, r)).collect();
        if k == 0 {
            let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
            let header: Vec<String> = std::iter::once("sample".to_owned())
                .chain(rows.iter().map(|r| quote(&if r.unit.is_empty() { r.name.clone() } else { format!("{} ({})", r.name, r.unit) })))
                .collect();
            out += &header.join(",");
            out.push('\n');
        }
        let _ = write!(out, "{}", k + 1);
        for r in &rows {
            let _ = write!(out, ",{}", r.value);
        }
        out.push('\n');
    }
    Ok(out)
}

/// `--doc NAME`, or the first document that isn't a component.
pub(crate) fn choose_doc(project: &Project, name: Option<&str>) -> Result<DocId, String> {
    match name {
        Some(name) => project.find(name).ok_or_else(|| {
            let names: Vec<&str> = project.documents.iter().map(|d| d.name.as_str()).collect();
            format!("no document “{name}”; the project has: {}", names.join(", "))
        }),
        None => project
            .documents
            .iter()
            .find(|d| d.component.is_none())
            .or(project.documents.first())
            .map(|d| d.id)
            .ok_or_else(|| "the project is empty".into()),
    }
}

/// `shine simulate …` from the binary: prints, writes `--out`, and gives the exit status.
pub fn main(args: &[String]) -> i32 {
    let result = parse(args).map_err(Failure::Usage).and_then(|a| {
        let text = simulate(&a)?;
        match &a.out {
            Some(path) => std::fs::write(path, &text).map_err(|e| Failure::Usage(format!("can't write {path}: {e}"))),
            None => {
                print!("{text}");
                Ok(())
            }
        }
    });
    match result {
        Ok(()) => 0,
        Err(f) => {
            let (Failure::Usage(m) | Failure::Simulation(m)) = &f;
            eprintln!("shine simulate: {m}");
            if matches!(f, Failure::Usage(_)) {
                eprintln!("Try: shine simulate --help");
            }
            f.exit_code()
        }
    }
}

pub mod export;
pub mod layer;
pub mod network;

#[cfg(test)]
mod tests;
