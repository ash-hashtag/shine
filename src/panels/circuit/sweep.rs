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
    Parameter { item: Item, param: String },
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
    /// Resonances, for a wavelength sweep: their spacing, width and Q.
    pub spectrum: Option<Spectrum>,
}

/// The resonances in a spectrum: dips (a ring's through port) or peaks (its
/// drop port), each where the power is beyond halfway between the curve's
/// lowest and highest.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    /// Whether the resonances are dips (else peaks).
    pub dips: bool,
    /// Where each resonance is, in nm, in order.
    pub resonances: Vec<f64>,
    /// Free spectral range: the mean spacing of neighbouring resonances, nm.
    pub fsr_nm: Option<f64>,
    /// Full width at half depth (or height) of the strongest resonance, nm;
    /// none when the sweep has too few points across it.
    pub fwhm_nm: Option<f64>,
    /// Quality factor: wavelength over width.
    pub q: Option<f64>,
}

/// Find the resonances in `ys` (power) over `xs` (wavelengths, ascending).
pub fn spectrum(xs: &[f64], ys: &[f64]) -> Option<Spectrum> {
    let n = xs.len().min(ys.len());
    if n < 5 {
        return None;
    }
    let (lo, hi) = ys[..n].iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &y| (a.min(y), b.max(y)));
    // Under ~0.5 dB of contrast there is nothing to measure.
    if hi <= 0.0 || hi.is_nan() || (hi - lo) / hi < 0.1 {
        return None;
    }
    let half = (lo + hi) / 2.0;
    let mut sorted = ys[..n].to_vec();
    sorted.sort_by(f64::total_cmp);
    // Mostly high with narrow dips, or mostly low with narrow peaks.
    let dips = sorted[n / 2] > half;
    let inside = |y: f64| if dips { y < half } else { y > half };
    // Where the curve crosses `half` between samples k and k + 1.
    let cross = |k: usize| xs[k] + (half - ys[k]) / (ys[k + 1] - ys[k]) * (xs[k + 1] - xs[k]);
    let mut resonances = Vec::new();
    let mut widths: Vec<(f64, f64, f64)> = Vec::new(); // (width, centre, depth)
    let mut k = 0;
    while k < n {
        if !inside(ys[k]) {
            k += 1;
            continue;
        }
        let start = k;
        while k < n && inside(ys[k]) {
            k += 1;
        }
        let end = k - 1;
        let best = (start..=end).fold(start, |b, j| if (ys[j] < ys[b]) == dips { j } else { b });
        // Refine the centre with a parabola through the extreme and its neighbours.
        let centre = if best > 0 && best + 1 < n {
            let (a, b, c) = (ys[best - 1], ys[best], ys[best + 1]);
            let d = a - 2.0 * b + c;
            let shift = if d.abs() > 0.0 { 0.5 * (a - c) / d } else { 0.0 };
            xs[best] + shift.clamp(-1.0, 1.0) * (xs[best + 1] - xs[best - 1]) / 2.0
        } else {
            xs[best]
        };
        resonances.push(centre);
        // A width needs both edges inside the sweep and a few points across.
        if start > 0 && end + 1 < n && end - start >= 2 {
            widths.push((cross(end) - cross(start - 1), centre, (ys[best] - half).abs()));
        }
    }
    let fsr_nm = (resonances.len() >= 2).then(|| (resonances[resonances.len() - 1] - resonances[0]) / (resonances.len() - 1) as f64);
    let strongest = widths.iter().max_by(|a, b| a.2.total_cmp(&b.2));
    let fwhm_nm = strongest.map(|w| w.0.abs());
    let q = strongest.filter(|w| w.0.abs() > 0.0).map(|w| w.1 / w.0.abs());
    Some(Spectrum { dips, resonances, fsr_nm, fwhm_nm, q })
}

/// A sweepable parameter: name, unit, sensible range.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub unit: &'static str,
    pub min: f64,
    pub max: f64,
}

fn p(name: &str, unit: &'static str, min: f64, max: f64) -> Param {
    Param { name: name.to_owned(), unit, min, max }
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
                Builtin::Laser { .. } => vec![p("power", "mW", 0.0, 4.0), p("phase", "rad", 0.0, TAU), p("wavelength", "nm", 400.0, 1700.0)],
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
                Builtin::Value { .. } => vec![p("value", "", 0.0, 1.0)],
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

/// The component a box uses, if it's a component box.
fn component_of(layout: &Layout, item: Item) -> Option<DocId> {
    match item {
        Item::Box(id) => match layout.instance(id)?.block {
            BlockRef::Component(doc) => Some(doc),
            BlockRef::Builtin(_) => None,
        },
        Item::Part(_) => None,
    }
}

/// The parameters a component layout exports, under their exported names.
pub fn exported_params(component: &Layout) -> Vec<Param> {
    component
        .exports
        .iter()
        .filter_map(|e| {
            let p = params(component, e.item).into_iter().find(|p| p.name == e.param)?;
            Some(Param { name: e.name.clone(), ..p })
        })
        .collect()
}

/// [`params`], plus a component box's exported parameters.
pub fn params_in(layout: &Layout, item: Item, docs: &dyn Documents) -> Vec<Param> {
    match component_of(layout, item) {
        Some(doc) => docs.layout(doc).map(|(inner, _)| exported_params(inner)).unwrap_or_default(),
        None => params(layout, item),
    }
}

/// [`get`], including a component box's exported parameters: the box's own
/// value, or the one inside the component.
pub fn get_in(layout: &Layout, item: Item, param: &str, docs: &dyn Documents) -> Option<f64> {
    let Some(doc) = component_of(layout, item) else { return get(layout, item, param) };
    let Item::Box(id) = item else { return None };
    let (inner, _) = docs.layout(doc)?;
    let export = inner.exports.iter().find(|e| e.name == param)?;
    layout.instance(id)?.param(param).or_else(|| get(inner, export.item, &export.param))
}

/// Set a parameter; returns false if the item has no such parameter. On a
/// component box it sets the box's value for an exported parameter.
pub fn set(layout: &mut Layout, item: Item, param: &str, value: f64) -> bool {
    if component_of(layout, item).is_some() {
        let Item::Box(id) = item else { return false };
        if let Some(inst) = layout.instances.iter_mut().find(|i| i.id == id) {
            inst.set_param(param, Some(value));
            return true;
        }
        return false;
    }
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
                (Builtin::Laser { wavelength_nm, .. }, "wavelength") => f(wavelength_nm),
                (Builtin::Value { value, .. }, "value") => f(value),
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
pub fn outputs(layout: &Layout, outcome: &Outcome, name_of_box: &dyn Fn(InstanceId) -> String) -> Vec<(String, f64, bool)> {
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
            let unit = params_in(layout, *item, docs).iter().find(|p| p.name == *param).map_or("", |p| p.unit);
            (*item, param.as_str(), if unit.is_empty() { param.to_string() } else { format!("{param} ({unit})") })
        }
    };
    let mut result = SweepResult { x_label, ..SweepResult::default() };
    if get_in(layout, item, param, docs).is_none() {
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

/// A second swept parameter: with a sweep along x, a map over (x, y).
#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    pub item: Item,
    pub param: String,
    pub start: f64,
    pub stop: f64,
    pub points: usize,
}

/// One output over a map: `power[row][column]`, rows along y, columns along x.
#[derive(Clone, Debug, PartialEq)]
pub struct MapSeries {
    pub name: String,
    pub power: Vec<Vec<f64>>,
    pub from_box: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapResult {
    pub x_label: String,
    pub y_label: String,
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    pub series: Vec<MapSeries>,
    pub error: Option<String>,
}

/// Sweep `spec` along x at each value of `y`: every point a full simulation.
#[allow(clippy::too_many_arguments)]
pub fn run_map(
    layout: &Layout,
    cell_um: f64,
    docs: &(dyn Documents + Sync),
    machine: &Machine,
    root: Option<DocId>,
    spec: &SweepSpec,
    y: &Axis,
    name_of_box: &(dyn Fn(InstanceId) -> String + Sync),
) -> MapResult {
    let unit = params_in(layout, y.item, docs).iter().find(|p| p.name == y.param).map_or("", |p| p.unit);
    let y_label = if unit.is_empty() { y.param.clone() } else { format!("{} ({unit})", y.param) };
    let mut map = MapResult { y_label, ..MapResult::default() };
    if get_in(layout, y.item, &y.param, docs).is_none() {
        map.error = Some("Nothing to vary: pick a part or block with that parameter".into());
        return map;
    }
    for value in values(y.start, y.stop, y.points.clamp(2, 1001)) {
        let mut l = layout.clone();
        set(&mut l, y.item, &y.param, value);
        let row = run(&l, cell_um, docs, machine, root, spec, name_of_box);
        map.x_label = row.x_label.clone();
        if let Some(e) = row.error {
            map.error = Some(format!("At {} = {value:.4}: {e}", y.param));
            break;
        }
        map.xs = row.xs;
        for s in row.series {
            match map.series.iter_mut().find(|m| m.name == s.name) {
                Some(m) => m.power.push(s.power),
                None => {
                    // An output that appeared mid-map starts with dark rows.
                    let mut power = vec![vec![0.0; s.power.len()]; map.ys.len()];
                    power.push(s.power);
                    map.series.push(MapSeries { name: s.name, power, from_box: s.from_box });
                }
            }
        }
        map.ys.push(value);
        let (rows, columns) = (map.ys.len(), map.xs.len());
        for m in &mut map.series {
            m.power.resize(rows, vec![0.0; columns]);
        }
    }
    map
}

impl MapResult {
    /// The map as CSV, one row per point: x, y, then each output's power (mW).
    pub fn to_csv(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = std::iter::once(quote(&self.x_label))
            .chain(std::iter::once(quote(&self.y_label)))
            .chain(self.series.iter().map(|s| quote(&format!("{} power (mW)", s.name))))
            .collect::<Vec<_>>()
            .join(",");
        out.push('\n');
        for (r, y) in self.ys.iter().enumerate() {
            for (c, x) in self.xs.iter().enumerate() {
                out += &format!("{x},{y}");
                for s in &self.series {
                    out += &format!(",{}", s.power[r][c]);
                }
                out.push('\n');
            }
        }
        out
    }
}

impl SweepResult {
    /// Whether the x axis is wavelength (so resonances can be measured).
    pub fn is_wavelength(&self) -> bool {
        self.x_label.starts_with("Wavelength")
    }

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
                    spectrum: self.is_wavelength().then(|| spectrum(&self.xs, &s.power)).flatten(),
                }
            })
            .collect()
    }

    /// Power relative to the input at each point, in dB.
    pub fn relative_db(&self, series: &Series) -> Vec<f64> {
        series.power.iter().zip(&self.reference).map(|(&p, &r)| if r > 0.0 { db(p / r) } else { DB_FLOOR }).collect()
    }

    /// The readouts as CSV, one row per output.
    pub fn readouts_csv(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let opt = |v: Option<f64>| v.map_or_else(String::new, |v| format!("{v}"));
        let mut out = format!(
            "\"output\",\"peak (mW)\",{},\"minimum (mW)\",\"minimum at\",\"insertion loss (dB)\",\"extinction (dB)\",\"resonances\",\"FSR (nm)\",\"width (nm)\",\"Q\"\n",
            quote(&format!("peak at ({})", self.x_label))
        );
        for r in self.readouts() {
            let s = r.spectrum.as_ref();
            out += &format!(
                "{},{},{},{},{},{},{},{},{},{},{}\n",
                quote(&r.name),
                r.peak,
                r.peak_at,
                r.min,
                r.min_at,
                opt(r.insertion_loss_db),
                r.extinction_db,
                s.map_or_else(String::new, |s| format!("{} {}", s.resonances.len(), if s.dips { "dips" } else { "peaks" })),
                opt(s.and_then(|s| s.fsr_nm)),
                opt(s.and_then(|s| s.fwhm_nm)),
                opt(s.and_then(|s| s.q)),
            );
        }
        out
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
