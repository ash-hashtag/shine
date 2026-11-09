//! Sweeps: simulate a design over a range of wavelengths or of one
//! parameter, and collect the power at every output.
//!
//! Each point is a full simulation (light and electronics settled), so
//! sweeps work on any design, including components and feedback.

use super::sim::{Documents, Machine, Options, Outcome, simulate};
use crate::panels::optics::model::{BlockRef, Builtin, InstanceId, Item, Layout, OpticComponentKind};
use crate::panels::project::DocId;

/// What varies along the sweep.
#[derive(Clone, Debug, PartialEq)]
pub enum SweepKind {
    /// Tune one laser (by part index) across wavelengths in nm.
    Wavelength { laser: usize },
    /// Step one parameter of a part or a built-in block.
    Parameter { item: Item, param: &'static str },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SweepSpec {
    pub kind: SweepKind,
    pub start: f64,
    pub stop: f64,
    pub points: usize,
}

/// One curve: power at an output for every swept value.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub name: String,
    pub power: Vec<f64>,
    /// A light output of a box, rather than a detector or output port.
    pub from_box: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SweepResult {
    /// What the x axis is, with its unit.
    pub x_label: String,
    pub xs: Vec<f64>,
    pub series: Vec<Series>,
    /// Input power at each point, in mW: the tuned laser's for a wavelength
    /// sweep, all top-level lasers' otherwise. Relative levels and
    /// insertion loss are measured against it.
    pub reference: Vec<f64>,
    /// The first point that failed to simulate, if any.
    pub error: Option<String>,
}

/// Figures of merit of one curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Readout {
    pub name: String,
    /// Highest power (mW) and where.
    pub peak: f64,
    pub peak_at: f64,
    /// Lowest power (mW) and where.
    pub min: f64,
    pub min_at: f64,
    /// Loss from the input to the peak, in dB (none without an input).
    pub insertion_loss_db: Option<f64>,
    /// Peak over minimum, in dB.
    pub extinction_db: f64,
}

/// A sweepable parameter: name, unit, sensible range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Param {
    pub name: &'static str,
    pub unit: &'static str,
    pub min: f64,
    pub max: f64,
}

const fn p(name: &'static str, unit: &'static str, min: f64, max: f64) -> Param {
    Param { name, unit, min, max }
}

const TAU: f64 = std::f64::consts::TAU;

/// Parameters of an item that a sweep can vary.
pub fn params(layout: &Layout, item: Item) -> Vec<Param> {
    match item {
        Item::Part(i) => match layout.components.get(i).map(|c| c.kind) {
            Some(OpticComponentKind::Laser { .. }) => {
                vec![p("power", "mW", 0.0, 4.0), p("phase", "rad", 0.0, TAU), p("wavelength", "nm", 400.0, 1700.0)]
            }
            Some(OpticComponentKind::BeamSplitter { .. }) => vec![p("reflectivity", "", 0.0, 1.0)],
            Some(OpticComponentKind::PhaseShifter { .. }) => vec![p("phase", "rad", 0.0, TAU)],
            Some(OpticComponentKind::Attenuator { .. }) => vec![p("transmission", "", 0.0, 1.0)],
            _ => Vec::new(),
        },
        Item::Box(id) => match layout.instance(id).map(|i| &i.block) {
            Some(BlockRef::Builtin(b)) => match b {
                Builtin::Laser { .. } => vec![p("power", "mW", 0.0, 4.0), p("phase", "rad", 0.0, TAU)],
                Builtin::Coupler { .. } => vec![p("coupling", "", 0.0, 1.0)],
                Builtin::PhaseShifter { .. } => vec![p("phase", "rad", 0.0, TAU)],
                Builtin::Attenuator { .. } => vec![p("loss", "dB", 0.0, 30.0)],
                Builtin::DelayLine { .. } => vec![
                    p("length", "µm", 0.0, 1000.0),
                    p("n_eff", "", 1.0, 4.0),
                    p("n_g", "", 1.0, 6.0),
                    p("loss", "dB/cm", 0.0, 100.0),
                ],
                Builtin::Mzi { .. } => vec![p("θ", "rad", 0.0, TAU), p("φ", "rad", 0.0, TAU)],
                Builtin::Ring { .. } | Builtin::AddDropRing { .. } => vec![
                    p("coupling", "", 0.0, 1.0),
                    p("length", "µm", 1.0, 200.0),
                    p("n_eff", "", 1.0, 4.0),
                    p("n_g", "", 1.0, 6.0),
                    p("loss", "dB/cm", 0.0, 100.0),
                ],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        },
    }
}

/// Current value of a parameter.
pub fn get(layout: &Layout, item: Item, param: &str) -> Option<f64> {
    let mut copy = layout.clone();
    let mut value = None;
    visit(&mut copy, item, param, &mut |v| value = Some(*v));
    value
}

/// Set a parameter; returns false if the item has no such parameter.
pub fn set(layout: &mut Layout, item: Item, param: &str, value: f64) -> bool {
    let mut found = false;
    visit(layout, item, param, &mut |v| {
        *v = value;
        found = true;
    });
    found
}

/// Calls `f` with the parameter as an f64, writing back what it leaves.
fn visit(layout: &mut Layout, item: Item, param: &str, f: &mut dyn FnMut(&mut f64)) {
    let mut f32_field = |field: &mut f32| {
        let mut v = *field as f64;
        f(&mut v);
        *field = v as f32;
    };
    match item {
        Item::Part(i) => {
            let Some(c) = layout.components.get_mut(i) else { return };
            match (&mut c.kind, param) {
                (OpticComponentKind::Laser { amplitude, .. }, "power") => {
                    let mut a = *amplitude as f64;
                    as_power(&mut a, f);
                    *amplitude = a as f32;
                }
                (OpticComponentKind::Laser { phase, .. } | OpticComponentKind::PhaseShifter { phase }, "phase") => f32_field(phase),
                (OpticComponentKind::Laser { wavelength, .. }, "wavelength") => f32_field(wavelength),
                (OpticComponentKind::BeamSplitter { reflectivity, transparency, .. }, "reflectivity") => {
                    f32_field(reflectivity);
                    *reflectivity = reflectivity.clamp(0.0, 1.0);
                    *transparency = 1.0 - *reflectivity;
                }
                (OpticComponentKind::Attenuator { transmission }, "transmission") => f32_field(transmission),
                _ => {}
            }
        }
        Item::Box(id) => {
            let Some(inst) = layout.instances.iter_mut().find(|i| i.id == id) else { return };
            let BlockRef::Builtin(b) = &mut inst.block else { return };
            match (b, param) {
                (Builtin::Laser { amplitude, .. }, "power") => as_power(amplitude, f),
                (Builtin::Laser { phase, .. } | Builtin::PhaseShifter { phase }, "phase") => f(phase),
                (Builtin::Coupler { coupling }, "coupling") => f(coupling),
                (Builtin::Attenuator { loss_db }, "loss") => f(loss_db),
                (Builtin::DelayLine { length_um, .. }, "length") => f(length_um),
                (Builtin::DelayLine { n_eff, .. }, "n_eff") => f(n_eff),
                (
                    Builtin::DelayLine { n_g, .. } | Builtin::Ring { n_g, .. } | Builtin::AddDropRing { n_g, .. },
                    "n_g",
                ) => f(n_g),
                (Builtin::DelayLine { loss_db_per_cm, .. }, "loss") => f(loss_db_per_cm),
                (Builtin::Mzi { theta, .. }, "θ") => f(theta),
                (Builtin::Mzi { phi, .. }, "φ") => f(phi),
                (Builtin::Ring { coupling, .. } | Builtin::AddDropRing { coupling, .. }, "coupling") => f(coupling),
                (Builtin::Ring { length_um, .. } | Builtin::AddDropRing { length_um, .. }, "length") => f(length_um),
                (Builtin::Ring { n_eff, .. } | Builtin::AddDropRing { n_eff, .. }, "n_eff") => f(n_eff),
                (Builtin::Ring { loss_db_per_cm, .. } | Builtin::AddDropRing { loss_db_per_cm, .. }, "loss") => {
                    f(loss_db_per_cm)
                }
                _ => {}
            }
        }
    }
}

/// Offers a field amplitude to `f` as its power in mW.
fn as_power(amplitude: &mut f64, f: &mut dyn FnMut(&mut f64)) {
    let mut mw = *amplitude * *amplitude;
    f(&mut mw);
    *amplitude = mw.max(0.0).sqrt();
}

/// Nominal power of a design's top-level lasers, in mW (before any signal
/// switches them): the input that relative levels are measured against.
pub fn source_power(layout: &Layout) -> f64 {
    let parts = layout.components.iter().filter_map(|c| match c.kind {
        OpticComponentKind::Laser { amplitude, .. } => Some((amplitude as f64).powi(2)),
        _ => None,
    });
    let boxes = layout.instances.iter().filter_map(|i| match &i.block {
        BlockRef::Builtin(Builtin::Laser { amplitude, .. }) => Some(amplitude * amplitude),
        _ => None,
    });
    parts.chain(boxes).sum()
}

/// Everything in a design worth plotting: detectors and output ports (by
/// part), and every light output of top-level boxes.
fn outputs(layout: &Layout, outcome: &Outcome, name_of_box: &dyn Fn(InstanceId) -> String) -> Vec<(String, f64, bool)> {
    let mut out: Vec<(String, f64, bool)> = outcome
        .detector_power
        .iter()
        .map(|&(i, power)| {
            let c = &layout.components[i];
            let (x, y) = crate::panels::optics::model::cell_of(c.pos);
            let kind = if matches!(c.kind, OpticComponentKind::OutputPort) { "Output" } else { "Detector" };
            (layout.label(i).map_or_else(|| format!("{kind} {x},{y}"), str::to_owned), power, false)
        })
        .collect();
    let mut ports: Vec<(&(InstanceId, usize), &f64)> = outcome.port_power.iter().collect();
    ports.sort_by_key(|(k, _)| **k);
    out.extend(ports.into_iter().map(|((id, k), p)| {
        let at = layout.instance(*id).map_or_else(String::new, |i| format!(" {},{}", i.cell.0, i.cell.1));
        (format!("{}{at} out{}", name_of_box(*id), k + 1), *p, true)
    }));
    // Names must be unique: each is one curve.
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for (name, _, _) in &mut out {
        let n = seen.entry(name.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            *name = format!("{name} ({n})");
        }
    }
    out
}

/// Evenly spaced values from `start` to `stop` inclusive.
pub fn values(start: f64, stop: f64, points: usize) -> Vec<f64> {
    let n = points.max(2);
    (0..n).map(|k| start + (stop - start) * k as f64 / (n - 1) as f64).collect()
}

/// Run a sweep on `layout`. `name_of_box` names top-level boxes for the legend.
pub fn run(
    layout: &Layout,
    cell_um: f64,
    docs: &(dyn Documents + Sync),
    machine: &Machine,
    root: Option<DocId>,
    spec: &SweepSpec,
    name_of_box: &(dyn Fn(InstanceId) -> String + Sync),
) -> SweepResult {
    let (item, param, x_label) = match &spec.kind {
        SweepKind::Wavelength { laser } => (Item::Part(*laser), "wavelength", "Wavelength (nm)".to_owned()),
        SweepKind::Parameter { item, param } => {
            let unit = params(layout, *item).iter().find(|p| p.name == *param).map_or("", |p| p.unit);
            (*item, *param, if unit.is_empty() { param.to_string() } else { format!("{param} ({unit})") })
        }
    };
    let mut result = SweepResult { x_label, ..SweepResult::default() };
    if get(layout, item, param).is_none() {
        result.error = Some("Nothing to sweep: pick a laser or a part with that parameter".into());
        return result;
    }
    // Each point is independent: simulate them in parallel (in order on the
    // web, which has no threads), then assemble the curves in order.
    let xs = values(spec.start, spec.stop, spec.points);
    let points = crate::photonics::par::map(&xs, |&x| {
        let mut l = layout.clone();
        set(&mut l, item, param, x);
        let outcome = simulate(&l, cell_um, docs, machine, root, &Options::default());
        if let Some(e) = outcome.error {
            return Err(format!("At {x:.4}: {e}"));
        }
        let reference = match spec.kind {
            SweepKind::Wavelength { laser } => match l.components[laser].kind {
                OpticComponentKind::Laser { amplitude, .. } => (amplitude as f64).powi(2),
                _ => 0.0,
            },
            SweepKind::Parameter { .. } => source_power(&l),
        };
        Ok((outputs(&l, &outcome, name_of_box), reference))
    });
    for (x, point) in xs.into_iter().zip(points) {
        let (outputs, reference) = match point {
            Ok(point) => point,
            Err(e) => {
                result.error = Some(e);
                break;
            }
        };
        for (name, power, from_box) in outputs {
            match result.series.iter_mut().find(|s| s.name == name) {
                Some(s) => s.power.push(power),
                None => {
                    // An output that appeared mid-sweep starts with zeros.
                    let mut series = Series { name, power: vec![0.0; result.xs.len()], from_box };
                    series.power.push(power);
                    result.series.push(series);
                }
            }
        }
        result.xs.push(x);
        result.reference.push(reference);
        for s in &mut result.series {
            s.power.resize(result.xs.len(), 0.0);
        }
    }
    result
}

impl SweepResult {
    /// Peak, minimum, insertion loss and extinction ratio of each curve.
    pub fn readouts(&self) -> Vec<Readout> {
        self.series
            .iter()
            .filter(|s| !s.power.is_empty())
            .map(|s| {
                let at = |pick: fn(f64, f64) -> bool| {
                    (0..s.power.len()).fold(0, |best, k| if pick(s.power[k], s.power[best]) { k } else { best })
                };
                let (hi, lo) = (at(|a, b| a > b), at(|a, b| a < b));
                let input = self.reference.get(hi).copied().filter(|&r| r > 0.0);
                Readout {
                    name: s.name.clone(),
                    peak: s.power[hi],
                    peak_at: self.xs[hi],
                    min: s.power[lo],
                    min_at: self.xs[lo],
                    insertion_loss_db: input.map(|r| db(r) - db(s.power[hi])),
                    extinction_db: db(s.power[hi]) - db(s.power[lo]),
                }
            })
            .collect()
    }

    /// Power relative to the input at each point, in dB.
    pub fn relative_db(&self, series: &Series) -> Vec<f64> {
        series.power.iter().zip(&self.reference).map(|(&p, &r)| if r > 0.0 { db(p / r) } else { DB_FLOOR }).collect()
    }

    /// The sweep as CSV: the x column, then one power column (mW) per output.
    pub fn to_csv(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = std::iter::once(quote(&self.x_label))
            .chain(self.series.iter().map(|s| quote(&format!("{} power (mW)", s.name))))
            .collect::<Vec<_>>()
            .join(",");
        out.push('\n');
        for (k, x) in self.xs.iter().enumerate() {
            out += &std::iter::once(format!("{x}"))
                .chain(self.series.iter().map(|s| format!("{}", s.power[k])))
                .collect::<Vec<_>>()
                .join(",");
            out.push('\n');
        }
        out
    }
}

pub use crate::panels::units::{DB_FLOOR, db};

#[cfg(test)]
mod tests;
