//! What a design does: one simulation, its transmission between ports,
//! sweeps, Monte Carlo, optimisation, and files for other tools.

use rand::SeedableRng;
use rand::rngs::StdRng;

use super::design::{Design, part_names};
use super::error::{Error, Result};
use super::part::{Part, Port, ports_of};
use super::project::lock;
use super::records::{Aim, Aimed, Complex, Direction, Goal, Matrix, Optimised, PortKind, Reading, Readouts, Series, Stats, Variation};
use crate::panels::circuit::sim::{self, Machine, Options, Outcome};
use crate::panels::circuit::{monte_carlo, sweep, transfer};
use crate::panels::interop;
use crate::panels::optics::model::{BlockRef, Builtin, Item, Layout, PortRef, Terminal};
use crate::panels::project::{DocId, layout_interface};

/// The power arriving at a box's light input `port`, along the waveguide
/// feeding it (0 when nothing does).
fn power_into(layout: &Layout, outcome: &Outcome, id: crate::panels::optics::model::InstanceId, port: usize) -> f64 {
    let at = PortRef { instance: id, port };
    layout
        .wires
        .iter()
        .find(|w| w.to == at)
        .and_then(|w| outcome.port_power.get(&(w.from.instance, w.from.port)).copied())
        .unwrap_or(0.0)
}

/// Every reading a design gives: each detector and light output's power
/// (under its part's name), and each part's light outputs ("Part.port").
fn readings(layout: &Layout, outcome: &Outcome, names: &[(Item, String)], port_names: &[(Item, Vec<String>)]) -> Vec<(String, f64)> {
    let name = |item: Item| names.iter().find(|(i, _)| *i == item).map_or_else(String::new, |(_, n)| n.clone());
    let mut out = Vec::new();
    for inst in &layout.instances {
        if matches!(inst.block, BlockRef::Builtin(Builtin::Detector | Builtin::OutputPin { .. })) {
            out.push((name(Item::Box(inst.id)), power_into(layout, outcome, inst.id, 0)));
        }
    }
    for inst in &layout.instances {
        let outputs = port_names.iter().find(|(i, _)| *i == Item::Box(inst.id)).map(|(_, p)| p.as_slice()).unwrap_or(&[]);
        for (k, port) in outputs.iter().enumerate() {
            let p = outcome.port_power.get(&(inst.id, k)).copied().unwrap_or(0.0);
            out.push((format!("{}.{port}", name(Item::Box(inst.id))), p));
        }
    }
    out
}

/// A design's state for its analyses: everything they need, copied out of
/// the project so the lock is held only briefly.
struct Snapshot {
    /// Copies of the components the design uses.
    project: sim::Snapshot,
    doc: DocId,
    layout: Layout,
    cell_um: f64,
    machine: Machine,
    names: Vec<(Item, String)>,
    /// Each part's light output names.
    outputs: Vec<(Item, Vec<String>)>,
}

impl Snapshot {
    fn of(design: &Design) -> Snapshot {
        let inner = lock(&design.inner);
        let d = inner.project.doc(design.id).expect("the design");
        let layout = d.layout().clone();
        let outputs = layout
            .instances
            .iter()
            .map(|i| {
                let item = Item::Box(i.id);
                let names = ports_of(&inner, design.id, item)
                    .into_iter()
                    .filter(|(p, _)| p.kind == PortKind::Light && p.direction == Direction::Out)
                    .map(|(p, _)| p.name)
                    .collect();
                (item, names)
            })
            .collect();
        Snapshot {
            project: sim::Snapshot::of(&layout, &inner.project),
            doc: design.id,
            cell_um: d.cell_um(),
            machine: d.panel().machine.clone(),
            names: part_names(&inner.project, design.id),
            outputs,
            layout,
        }
    }

    fn simulate(&self, layout: &Layout) -> Result<Outcome> {
        let o = sim::simulate(layout, self.cell_um, &self.project, &self.machine, Some(self.doc), &Options::default());
        match o.error {
            Some(e) => Err(Error::Simulation(e)),
            None => Ok(o),
        }
    }

    fn readings(&self, layout: &Layout, outcome: &Outcome) -> Vec<(String, f64)> {
        readings(layout, outcome, &self.names, &self.outputs)
    }

    fn item_of(&self, part: &Part) -> Item {
        part.item
    }
}

/// One simulation of a design: the light everywhere, and the signals.
pub struct Simulation {
    layout: Layout,
    outcome: Outcome,
    names: Vec<(Item, String)>,
    outputs: Vec<(Item, Vec<String>)>,
}

impl Simulation {
    /// Whether the light and the electronics settled on one state.
    pub fn converged(&self) -> bool {
        self.outcome.converged
    }

    /// The power arriving at a part: at a detector or a light output, or
    /// whatever reaches any part's light inputs, in mW.
    pub fn power_mw(&self, part: &Part) -> Result<f64> {
        let Item::Box(id) = part.item else {
            return Err(Error::Invalid(format!("“{}” is a signal pin: read it with signal()", part.name())));
        };
        let inputs = self.layout.instance(id).map_or(0, |i| match &i.block {
            BlockRef::Builtin(b) => b.port_counts().optical_in,
            BlockRef::Component(_) => usize::MAX,
        });
        let inputs = if inputs == usize::MAX { part.ports().iter().filter(|p| p.kind == PortKind::Light && p.direction == Direction::In).count() } else { inputs };
        if inputs == 0 {
            return Err(Error::Invalid(format!("“{}” has no light input: read what leaves it with output_power_mw()", part.name())));
        }
        Ok((0..inputs).map(|k| power_into(&self.layout, &self.outcome, id, k)).sum())
    }

    /// The power leaving a light output, in mW.
    pub fn output_power_mw(&self, port: &Port) -> Result<f64> {
        let (Item::Box(id), PortKind::Light, Direction::Out) = (port.part.item, port.info.kind, port.info.direction) else {
            return Err(Error::Invalid(format!("“{}” isn't a light output", port.label())));
        };
        Ok(self.outcome.port_power.get(&(id, port.index)).copied().unwrap_or(0.0))
    }

    /// A signal's value: what a signal output gives, or what arrives at a
    /// signal input.
    pub fn signal(&self, port: &Port) -> Result<f64> {
        if port.info.kind != PortKind::Signal {
            return Err(Error::Invalid(format!("“{}” carries light: read it with output_power_mw()", port.label())));
        }
        let t = Terminal { item: port.part.item, port: port.index };
        Ok(match port.info.direction {
            Direction::Out => self.outcome.signal(t.item, t.port),
            Direction::In => self.outcome.input(&self.layout, t),
        })
    }

    /// The number a display shows.
    pub fn display(&self, part: &Part) -> Result<u64> {
        match (part.item, part.item_block(&self.layout)) {
            (Item::Box(id), Some(Builtin::Display { bits })) => Ok(crate::panels::optics::input_number(&self.layout, &self.outcome, id, bits as usize)),
            _ => Err(Error::Invalid(format!("“{}” isn't a display", part.name()))),
        }
    }

    /// Every detector's and light output's power, and every part's light
    /// outputs ("Ring resonator.through"), in mW.
    pub fn readings(&self) -> Vec<Reading> {
        readings(&self.layout, &self.outcome, &self.names, &self.outputs).into_iter().map(|(name, power_mw)| Reading { name, power_mw }).collect()
    }
}

impl Part {
    fn item_block(&self, layout: &Layout) -> Option<Builtin> {
        let Item::Box(id) = self.item else { return None };
        match &layout.instance(id)?.block {
            BlockRef::Builtin(b) => Some(b.clone()),
            BlockRef::Component(_) => None,
        }
    }
}

/// How light passes between a design's light inputs and outputs (its pins),
/// at each wavelength.
pub struct Transmission {
    pub wavelengths_nm: Vec<f64>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    matrices: Vec<crate::photonics::Matrix>,
}

impl Transmission {
    fn index(names: &[String], name: &str, what: &str) -> Result<usize> {
        names.iter().position(|n| n == name).ok_or_else(|| Error::NoSuchPort { part: format!("the design's {what}s"), port: name.to_owned(), available: names.to_vec() })
    }

    /// The field leaving `output` per unit field entering `input`, at each wavelength.
    pub fn field(&self, output: &str, input: &str) -> Result<Vec<Complex>> {
        let (o, i) = (Self::index(&self.outputs, output, "output")?, Self::index(&self.inputs, input, "input")?);
        Ok(self.matrices.iter().map(|m| m[(o, i)].into()).collect())
    }

    /// The fraction of power from `input` reaching `output`, at each wavelength.
    pub fn power(&self, output: &str, input: &str) -> Result<Vec<f64>> {
        Ok(self.field(output, input)?.iter().map(Complex::norm_sqr).collect())
    }

    fn path(&self, output: &str, input: &str) -> Result<transfer::Path> {
        let t: Vec<crate::photonics::C64> = self.field(output, input)?.into_iter().map(Into::into).collect();
        Ok(transfer::path(&self.wavelengths_nm, &t))
    }

    /// The phase from `input` to `output`, unwrapped along the wavelengths, in radians.
    pub fn phase(&self, output: &str, input: &str) -> Result<Vec<f64>> {
        Ok(self.path(output, input)?.phase)
    }

    /// The group delay from `input` to `output`, in ps.
    pub fn group_delay_ps(&self, output: &str, input: &str) -> Result<Vec<f64>> {
        Ok(self.path(output, input)?.group_delay_ps)
    }

    /// The whole matrix (rows the outputs, columns the inputs) at one of the wavelengths.
    pub fn matrix(&self, wavelength_index: u32) -> Result<Matrix> {
        self.matrices
            .get(wavelength_index as usize)
            .map(Matrix::from_core)
            .ok_or_else(|| Error::Invalid(format!("wavelength {wavelength_index} is past the last of {}", self.matrices.len())))
    }
}

/// A sweep: every reading at each value of one parameter.
pub struct Sweep {
    pub x: Vec<f64>,
    /// What was swept, with its unit: "wavelength (nm)".
    pub x_label: String,
    series: Vec<Series>,
}

impl Sweep {
    /// The names of everything measured, as [`Simulation::readings`] names them.
    pub fn outputs(&self) -> Vec<String> {
        self.series.iter().map(|s| s.name.clone()).collect()
    }

    /// One output's power at every swept value.
    pub fn series(&self, output: &str) -> Result<Series> {
        self.series.iter().find(|s| s.name == output).cloned().ok_or_else(|| Error::NoSuchPort {
            part: "the sweep".into(),
            port: output.to_owned(),
            available: self.outputs(),
        })
    }

    /// An output's resonances: where they are, their spacing (FSR), width
    /// and Q. For sweeps of a laser's wavelength.
    pub fn readouts(&self, output: &str) -> Result<Readouts> {
        let s = self.series(output)?;
        let spectrum = sweep::spectrum(&self.x, &s.power_mw)
            .ok_or_else(|| Error::Invalid(format!("no resonances in “{output}”: sweep a laser's wavelength over a few of them")))?;
        let (max, min) = s.power_mw.iter().fold((0.0f64, f64::INFINITY), |(a, b), &p| (a.max(p), b.min(p)));
        Ok(Readouts {
            dips: spectrum.dips,
            resonances_nm: spectrum.resonances,
            fsr_nm: spectrum.fsr_nm,
            fwhm_nm: spectrum.fwhm_nm,
            q: spectrum.q,
            extinction_db: 10.0 * (max / min.max(1e-30)).log10(),
        })
    }

    /// The sweep as CSV: the swept value, then one column per output.
    pub fn to_csv(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = std::iter::once(quote(&self.x_label)).chain(self.series.iter().map(|s| quote(&format!("{} (mW)", s.name)))).collect::<Vec<_>>().join(",");
        out.push('\n');
        for (k, x) in self.x.iter().enumerate() {
            let row: Vec<String> = std::iter::once(x.to_string()).chain(self.series.iter().map(|s| s.power_mw[k].to_string())).collect();
            out += &row.join(",");
            out.push('\n');
        }
        out
    }
}

/// A design under fabrication variation: every reading over many runs.
pub struct MonteCarlo {
    pub runs: u32,
    pub seed: u64,
    series: Vec<Series>,
}

impl MonteCarlo {
    pub fn outputs(&self) -> Vec<String> {
        self.series.iter().map(|s| s.name.clone()).collect()
    }

    /// An output's power in every run, in mW.
    pub fn values(&self, output: &str) -> Result<Vec<f64>> {
        self.series.iter().find(|s| s.name == output).map(|s| s.power_mw.clone()).ok_or_else(|| Error::NoSuchPort {
            part: "the Monte Carlo runs".into(),
            port: output.to_owned(),
            available: self.outputs(),
        })
    }

    /// An output's mean, standard deviation and extremes over the runs.
    pub fn stats(&self, output: &str) -> Result<Stats> {
        let v = self.values(output)?;
        let n = v.len().max(1) as f64;
        let mean = v.iter().sum::<f64>() / n;
        let std = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt();
        let (min, max) = v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
        Ok(Stats { mean, std, min, max })
    }

    /// The fraction of runs whose output is between `min_mw` and `max_mw`.
    pub fn yield_of(&self, output: &str, min_mw: f64, max_mw: f64) -> Result<f64> {
        let v = self.values(output)?;
        Ok(v.iter().filter(|&&p| p >= min_mw && p <= max_mw).count() as f64 / v.len().max(1) as f64)
    }

    pub fn to_csv(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = std::iter::once("\"run\"".to_owned()).chain(self.series.iter().map(|s| quote(&format!("{} (mW)", s.name)))).collect::<Vec<_>>().join(",");
        out.push('\n');
        for k in 0..self.runs as usize {
            let row: Vec<String> = std::iter::once((k + 1).to_string()).chain(self.series.iter().map(|s| s.power_mw[k].to_string())).collect();
            out += &row.join(",");
            out.push('\n');
        }
        out
    }
}

/// Series from per-point readings, in the order the first point gives them.
fn to_series(points: Vec<Vec<(String, f64)>>) -> Vec<Series> {
    let mut series: Vec<Series> = points.first().map(|p| p.iter().map(|(n, _)| Series { name: n.clone(), power_mw: Vec::new() }).collect()).unwrap_or_default();
    for point in points {
        for s in &mut series {
            s.power_mw.push(point.iter().find(|(n, _)| *n == s.name).map_or(0.0, |r| r.1));
        }
    }
    series
}

impl Design {
    /// Simulate the design: lasers on, electronics settled.
    pub fn simulate(&self) -> Result<Simulation> {
        let s = Snapshot::of(self);
        let outcome = s.simulate(&s.layout)?;
        Ok(Simulation { layout: s.layout, outcome, names: s.names, outputs: s.outputs })
    }

    /// How light passes from each of the design's light inputs to each light
    /// output (its pins), at each wavelength, with its own lasers off.
    pub fn transmission(&self, wavelengths_nm: &[f64]) -> Result<Transmission> {
        let s = Snapshot::of(self);
        let t = sim::transmission(&s.layout, s.cell_um, &s.project, &s.machine, Some(s.doc), wavelengths_nm).map_err(Error::Simulation)?;
        let iface = layout_interface(&s.layout);
        Ok(Transmission { wavelengths_nm: t.wavelengths_nm, inputs: iface.inputs, outputs: iface.outputs, matrices: t.matrices })
    }

    /// Sweep one parameter of one part from `start` to `stop` in `points`
    /// steps, simulating at each: every reading at every value. Points run
    /// in parallel.
    pub fn sweep(&self, part: &Part, param: &str, start: f64, stop: f64, points: u32) -> Result<Sweep> {
        let p = part.param(param)?;
        let s = Snapshot::of(self);
        let item = s.item_of(part);
        let x = sweep::values(start, stop, points as usize);
        let results = crate::photonics::par::map(&x, |&v| {
            let mut l = s.layout.clone();
            sweep::set(&mut l, item, param, v);
            s.simulate(&l).map(|o| s.readings(&l, &o))
        });
        let points = results.into_iter().collect::<Result<Vec<_>>>()?;
        let x_label = if p.unit.is_empty() { param.to_owned() } else { format!("{param} ({})", p.unit) };
        Ok(Sweep { x, x_label, series: to_series(points) })
    }

    /// Run the design `runs` times with its parameters varied as fabrication
    /// would (`variation`'s standard deviations), from `seed` so it repeats.
    pub fn monte_carlo(&self, runs: u32, seed: u64, variation: &Variation) -> Result<MonteCarlo> {
        let v = monte_carlo::Variation { n_eff: variation.n_eff, coupling: variation.coupling, phase: variation.phase, loss: variation.loss };
        let s = Snapshot::of(self);
        let runs_list: Vec<u64> = (0..runs as u64).collect();
        let results = crate::photonics::par::map(&runs_list, |&k| {
            let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(k));
            let l = monte_carlo::vary(&s.layout, &v, &mut rng);
            s.simulate(&l).map(|o| s.readings(&l, &o))
        });
        let points = results.into_iter().collect::<Result<Vec<_>>>()?;
        Ok(MonteCarlo { runs, seed, series: to_series(points) })
    }

    /// Adjust parameters (part, parameter, from, to) to bring an output's
    /// power (as [`Simulation::readings`] names it) towards `goal`, by
    /// Nelder–Mead from the current values. Desktop only.
    pub fn optimise(&self, variables: &[(Part, String, f64, f64)], output: &str, goal: Goal, steps: u32) -> Result<Optimised> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (variables, output, goal, steps);
            Err(Error::Unsupported("optimisation runs in the desktop library".into()))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::panels::circuit::optimise::{self, Variable};
            for (part, param, _, _) in variables {
                part.param(param)?;
            }
            let s = Snapshot::of(self);
            let vars: Vec<Variable> = variables.iter().map(|(p, param, min, max)| Variable { item: p.item, param: param.clone(), min: *min, max: *max }).collect();
            let goal = match goal {
                Goal::Maximise => optimise::Goal::Maximise,
                Goal::Minimise => optimise::Goal::Minimise,
                Goal::Target(t) => optimise::Goal::Target(t),
            };
            let measure = |l: &Layout, o: &Outcome| {
                let r = s.readings(l, o);
                r.iter().find(|(n, _)| n == output).map(|r| r.1).ok_or_else(|| {
                    format!("no output “{output}”; there are: {}", r.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "))
                })
            };
            let r = optimise::optimise_measured(&s.layout, s.cell_um, &s.project, &s.machine, Some(s.doc), &vars, goal, steps as u64, &measure);
            if let Some(e) = r.error {
                return Err(Error::Simulation(e));
            }
            Ok(Optimised { values: r.values, before_mw: r.before, after_mw: r.after, simulations: r.simulations as u32 })
        }
    }

    /// Adjust parameters (part, parameter, from, to) to bring several
    /// outputs towards their goals at once, weighted and summed, each as
    /// the design is or on average across a band of wavelengths. Desktop
    /// only.
    pub fn optimise_aims(&self, variables: &[(Part, String, f64, f64)], aims: &[Aim], steps: u32) -> Result<Aimed> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (variables, aims, steps);
            Err(Error::Unsupported("optimisation runs in the desktop library".into()))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::panels::circuit::optimise::{self, Band, Target, Variable};
            for (part, param, _, _) in variables {
                part.param(param)?;
            }
            let s = Snapshot::of(self);
            let vars: Vec<Variable> = variables.iter().map(|(p, param, min, max)| Variable { item: p.item, param: param.clone(), min: *min, max: *max }).collect();
            let targets: Vec<Target> = aims
                .iter()
                .map(|a| Target {
                    output: a.output.clone(),
                    goal: match a.goal {
                        Goal::Maximise => optimise::Goal::Maximise,
                        Goal::Minimise => optimise::Goal::Minimise,
                        Goal::Target(t) => optimise::Goal::Target(t),
                    },
                    band: a.band.map(|(from_nm, to_nm, points)| Band { from_nm, to_nm, points: points as usize }),
                    weight: a.weight,
                })
                .collect();
            let read = |l: &Layout, o: &Outcome| s.readings(l, o);
            let r = optimise::optimise_targets(&s.layout, s.cell_um, &s.project, &s.machine, Some(s.doc), &vars, &targets, steps as u64, &read);
            if let Some(e) = r.error {
                return Err(Error::Simulation(e));
            }
            Ok(Aimed { values: r.values, before_mw: r.before, after_mw: r.after, simulations: r.simulations as u32 })
        }
    }

    /// Apply values an optimisation found.
    pub fn apply(&self, variables: &[(Part, String, f64, f64)], values: &[f64]) -> Result<()> {
        for ((part, param, _, _), v) in variables.iter().zip(values) {
            part.set(param, *v)?;
        }
        Ok(())
    }

    // ---- files for other tools ------------------------------------------------------

    /// Its S-parameters between its light pins, as a Touchstone file.
    pub fn to_touchstone(&self, wavelengths_nm: &[f64]) -> Result<String> {
        let inner = lock(&self.inner);
        interop::touchstone(&inner.project, self.id, wavelengths_nm, "").map(|t| t.0).map_err(Error::Simulation)
    }

    /// Its circuit as a SAX netlist with models (Python).
    pub fn to_sax(&self, wavelengths_nm: &[f64]) -> Result<String> {
        let inner = lock(&self.inner);
        interop::sax::export(&inner.project, self.id, wavelengths_nm, "").map(|e| e.python).map_err(Error::Simulation)
    }

    /// Its layout for gdsfactory (YAML), which draws the mask.
    pub fn to_gdsfactory(&self, spacing_um: f64) -> Result<String> {
        let inner = lock(&self.inner);
        interop::gdsfactory::export(&inner.project, self.id, spacing_um, "").map(|e| e.yaml).map_err(Error::Simulation)
    }

    /// Open the desktop app on the project, showing this design: saved to a
    /// temporary file, and the app (`shine`, or the program in the
    /// `SHINE_APP` environment variable) started on it.
    pub fn show(&self) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            Err(Error::Unsupported("show() opens the desktop app; in the browser, save the project and open it at shine.lupyd.com/app".into()))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let project = super::Project { inner: self.inner.clone() };
            let file = std::env::temp_dir().join(format!("shine-{}-{}.shine", std::process::id(), crate::panels::names::slug(&self.name())));
            project.save(&file.to_string_lossy())?;
            let app = std::env::var("SHINE_APP").unwrap_or_else(|_| "shine".into());
            std::process::Command::new(&app).arg(&file).arg("--doc").arg(self.name()).spawn().map(|_| ()).map_err(|e| {
                Error::Unsupported(format!(
                    "couldn't start the Shine app ({app}: {e}). Install it from https://shine.lupyd.com, or set SHINE_APP; the project is saved at {}",
                    file.display()
                ))
            })
        }
    }
}
