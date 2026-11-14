//! Optimisation: find the parameter values (couplings, lengths, phases…)
//! that make an output reach a goal, by Nelder–Mead (from `argmin`), which
//! needs no derivatives: each step is a full simulation. It finds a local
//! optimum, starting from the design's current values.
//!
//! [`optimise_targets`] aims for several things at once: each target an
//! output's power towards a goal, at the design's own wavelengths or across
//! a band (its first laser tuned point by point, the goal met on average),
//! weighted and summed. So a filter's response can be shaped over a band,
//! or one output maximised while another is kept dark.

use argmin::core::{CostFunction, Error, Executor, State};
use argmin::solver::neldermead::NelderMead;

use super::sim::{Documents, Machine, Options, simulate};
use super::sweep::{get_in, outputs, set};
use crate::panels::optics::model::{InstanceId, Item, Layout};
use crate::panels::project::DocId;

/// A parameter to adjust, within bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub item: Item,
    pub param: String,
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Goal {
    Maximise,
    Minimise,
    /// Reach this power, in mW.
    Target(f64),
}

/// What to optimise: an output's power, towards a goal.
#[derive(Clone, Debug, PartialEq)]
pub struct Objective {
    pub output: String,
    pub goal: Goal,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Optimised {
    /// The best values found, one per variable.
    pub values: Vec<f64>,
    /// The output's power before and after, in mW.
    pub before: f64,
    pub after: f64,
    pub simulations: usize,
    pub error: Option<String>,
}

struct Problem<'a> {
    layout: &'a Layout,
    cell_um: f64,
    docs: &'a (dyn Documents + Sync),
    machine: &'a Machine,
    root: Option<DocId>,
    vars: &'a [Variable],
    goal: Goal,
    measure: &'a Measure<'a>,
    count: std::sync::atomic::AtomicUsize,
}

/// What an optimisation brings to its goal: a number from a simulated layout.
pub type Measure<'a> = dyn Fn(&Layout, &super::sim::Outcome) -> Result<f64, String> + Sync + 'a;

impl Problem<'_> {
    /// Values from a point of the unit cube (clamped into it).
    fn values(&self, u: &[f64]) -> Vec<f64> {
        self.vars.iter().zip(u).map(|(v, &x)| v.min + x.clamp(0.0, 1.0) * (v.max - v.min)).collect()
    }

    /// The output's power with these values.
    fn power(&self, values: &[f64]) -> Result<f64, String> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut l = self.layout.clone();
        for (v, &x) in self.vars.iter().zip(values) {
            set(&mut l, v.item, &v.param, x);
        }
        let o = simulate(&l, self.cell_um, self.docs, self.machine, self.root, &Options::default());
        if let Some(e) = o.error {
            return Err(e);
        }
        (self.measure)(&l, &o)
    }
}

/// The problem, lent to the optimiser (which takes what it's given).
struct Cost<'p, 'a>(&'p Problem<'a>);

impl CostFunction for Cost<'_, '_> {
    type Param = Vec<f64>;
    type Output = f64;

    fn cost(&self, u: &Vec<f64>) -> Result<f64, Error> {
        let this = self.0;
        let p = this.power(&this.values(u)).map_err(Error::msg)?;
        // Outside the bounds: the clamped value, plus a push back inside.
        let outside: f64 = u.iter().map(|&x| (x - x.clamp(0.0, 1.0)).powi(2)).sum();
        let cost = match this.goal {
            Goal::Maximise => -p,
            Goal::Minimise => p,
            Goal::Target(t) => (p - t).powi(2),
        };
        Ok(cost + 1e3 * outside)
    }
}

/// A band of wavelengths a target is met across, on average.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    pub from_nm: f64,
    pub to_nm: f64,
    pub points: usize,
}

/// One thing an optimisation aims for.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// The output, as sweeps and readings name it.
    pub output: String,
    pub goal: Goal,
    /// Across this band (the design's first laser tuned); None, as it is.
    pub band: Option<Band>,
    /// How much it counts against the other targets.
    pub weight: f64,
}

/// What [`optimise_targets`] found: the values, the total cost before and
/// after (lower is better), and each target's power before and after (mW,
/// the band's average for a band).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Response {
    pub values: Vec<f64>,
    pub cost_before: f64,
    pub cost_after: f64,
    pub before: Vec<f64>,
    pub after: Vec<f64>,
    pub simulations: usize,
    pub error: Option<String>,
}

/// The laser a band tunes: the layout's first, a part or a block.
pub fn tunable_laser(layout: &Layout) -> Option<Item> {
    use crate::panels::optics::model::{BlockRef, Builtin, OpticComponentKind};
    layout
        .components
        .iter()
        .position(|c| matches!(c.kind, OpticComponentKind::Laser { .. }))
        .map(Item::Part)
        .or_else(|| layout.instances.iter().find(|i| matches!(i.block, BlockRef::Builtin(Builtin::Laser { .. }))).map(|i| Item::Box(i.id)))
}

/// How far a power is from a goal: lower is better.
fn miss(goal: Goal, p: f64) -> f64 {
    match goal {
        Goal::Maximise => -p,
        Goal::Minimise => p,
        Goal::Target(t) => (p - t).powi(2),
    }
}

/// What a simulated layout's outputs read, by name, in mW.
pub type Read<'a> = dyn Fn(&Layout, &super::sim::Outcome) -> Vec<(String, f64)> + Sync + 'a;

/// Each target's power (mW, a band's average) and the weighted cost, for a
/// simulated layout; `simulations` counts the band's.
#[allow(clippy::too_many_arguments)]
fn evaluate(
    layout: &Layout,
    outcome: &super::sim::Outcome,
    cell_um: f64,
    docs: &(dyn Documents + Sync),
    machine: &Machine,
    root: Option<DocId>,
    targets: &[Target],
    read: &Read,
    simulations: &std::sync::atomic::AtomicUsize,
) -> Result<(Vec<f64>, f64), String> {
    let here = read(layout, outcome);
    let find = |there: &[(String, f64)], name: &str| {
        there.iter().find(|o| o.0 == name).map(|o| o.1).ok_or_else(|| {
            let names: Vec<&str> = there.iter().map(|o| o.0.as_str()).collect();
            format!("no output “{name}”; there are: {}", names.join(", "))
        })
    };
    let mut powers = Vec::with_capacity(targets.len());
    let mut cost = 0.0;
    for t in targets {
        let samples: Vec<f64> = match &t.band {
            None => vec![find(&here, &t.output)?],
            Some(b) => {
                // The first laser tuned to each wavelength of the band, in parallel.
                let laser = tunable_laser(layout).ok_or("a band needs a laser to tune; this design has none")?;
                let n = b.points.max(2);
                let wavelengths: Vec<f64> = (0..n).map(|k| b.from_nm + (b.to_nm - b.from_nm) * k as f64 / (n - 1) as f64).collect();
                simulations.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                crate::photonics::par::try_map(&wavelengths, |&wl| {
                    let mut l = layout.clone();
                    set(&mut l, laser, "wavelength", wl);
                    let o = simulate(&l, cell_um, docs, machine, root, &Options::default());
                    match o.error {
                        Some(e) => Err(e),
                        None => find(&read(&l, &o), &t.output),
                    }
                })?
            }
        };
        let n = samples.len().max(1) as f64;
        powers.push(samples.iter().sum::<f64>() / n);
        cost += t.weight * samples.iter().map(|&p| miss(t.goal, p)).sum::<f64>() / n;
    }
    Ok((powers, cost))
}

/// What a layout's outputs read, named as sweeps and the app name them.
pub fn read_outputs<'a>(names: &'a (dyn Fn(InstanceId) -> String + Sync)) -> impl Fn(&Layout, &super::sim::Outcome) -> Vec<(String, f64)> + Sync + 'a {
    move |l, o| outputs(l, o, names).into_iter().map(|(name, p, _)| (name, p)).collect()
}

/// Adjust `vars` to bring `targets` (weighted, summed) towards their goals;
/// `read` names and reads a simulated layout's outputs.
#[allow(clippy::too_many_arguments)]
pub fn optimise_targets(
    layout: &Layout,
    cell_um: f64,
    docs: &(dyn Documents + Sync),
    machine: &Machine,
    root: Option<DocId>,
    vars: &[Variable],
    targets: &[Target],
    max_steps: u64,
    read: &Read,
) -> Response {
    let mut response = Response::default();
    if targets.is_empty() {
        response.error = Some("Choose at least one output to aim for".into());
        return response;
    }
    let extra = std::sync::atomic::AtomicUsize::new(0);
    let measure = |l: &Layout, o: &super::sim::Outcome| evaluate(l, o, cell_um, docs, machine, root, targets, read, &extra).map(|r| r.1);
    let r = optimise_measured(layout, cell_um, docs, machine, root, vars, Goal::Minimise, max_steps, &measure);
    response.simulations = r.simulations + extra.load(std::sync::atomic::Ordering::Relaxed);
    if let Some(e) = r.error {
        response.error = Some(e);
        return response;
    }
    (response.values, response.cost_before, response.cost_after) = (r.values, r.before, r.after);
    // Each target before and after, to show.
    let each = |l: &Layout| -> Result<Vec<f64>, String> {
        let o = simulate(l, cell_um, docs, machine, root, &Options::default());
        if let Some(e) = o.error {
            return Err(e);
        }
        evaluate(l, &o, cell_um, docs, machine, root, targets, read, &extra).map(|r| r.0)
    };
    let mut tuned = layout.clone();
    for (v, &x) in vars.iter().zip(&response.values) {
        set(&mut tuned, v.item, &v.param, x);
    }
    match (each(layout), each(&tuned)) {
        (Ok(before), Ok(after)) => (response.before, response.after) = (before, after),
        (Err(e), _) | (_, Err(e)) => response.error = Some(e),
    }
    response
}

/// Adjust `vars` to bring the objective's output towards its goal.
#[allow(clippy::too_many_arguments)]
pub fn optimise(
    layout: &Layout,
    cell_um: f64,
    docs: &(dyn Documents + Sync),
    machine: &Machine,
    root: Option<DocId>,
    vars: &[Variable],
    objective: &Objective,
    max_steps: u64,
    names: &(dyn Fn(InstanceId) -> String + Sync),
) -> Optimised {
    let measure = |l: &Layout, o: &super::sim::Outcome| {
        outputs(l, o, names)
            .into_iter()
            .find(|out| out.0 == objective.output)
            .map(|out| out.1)
            .ok_or_else(|| format!("no output “{}”", objective.output))
    };
    optimise_measured(layout, cell_um, docs, machine, root, vars, objective.goal, max_steps, &measure)
}

/// Adjust `vars` to bring what `measure` reads from each simulation towards
/// `goal`.
#[allow(clippy::too_many_arguments)]
pub fn optimise_measured(
    layout: &Layout,
    cell_um: f64,
    docs: &(dyn Documents + Sync),
    machine: &Machine,
    root: Option<DocId>,
    vars: &[Variable],
    goal: Goal,
    max_steps: u64,
    measure: &Measure,
) -> Optimised {
    let mut result = Optimised::default();
    if vars.is_empty() {
        result.error = Some("Choose at least one parameter to adjust".into());
        return result;
    }
    let mut start = Vec::with_capacity(vars.len());
    for v in vars {
        let Some(x) = get_in(layout, v.item, &v.param, docs) else {
            result.error = Some(format!("Nothing has a parameter “{}” any more", v.param));
            return result;
        };
        let span = v.max - v.min;
        start.push(if span > 0.0 { ((x - v.min) / span).clamp(0.0, 1.0) } else { 0.0 });
    }
    let problem = Problem { layout, cell_um, docs, machine, root, vars, goal, measure, count: Default::default() };
    match problem.power(&problem.values(&start)) {
        Ok(p) => result.before = p,
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    }
    // The starting simplex: the current values, and a step along each.
    let mut simplex = vec![start.clone()];
    for k in 0..start.len() {
        let mut p = start.clone();
        p[k] = if p[k] + 0.15 <= 1.0 { p[k] + 0.15 } else { p[k] - 0.15 };
        simplex.push(p);
    }
    let solver = match NelderMead::new(simplex).with_sd_tolerance(1e-12) {
        Ok(s) => s,
        Err(e) => {
            result.error = Some(e.to_string());
            return result;
        }
    };
    let run = Executor::new(Cost(&problem), solver).configure(|s| s.max_iters(max_steps)).run();
    match run {
        Ok(r) => {
            let best = r.state().get_best_param().cloned().unwrap_or(start);
            result.values = problem.values(&best);
            result.after = problem.power(&result.values).unwrap_or(result.before);
        }
        Err(e) => result.error = Some(e.to_string()),
    }
    result.simulations = problem.count.load(std::sync::atomic::Ordering::Relaxed);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::circuit::sim::NoDocuments;
    use crate::panels::optics::model::{BlockRef, Builtin, OpticComponentKind, PlacedComponent, PortRef, cell_center};
    use emath::Vec2;

    fn names(id: InstanceId) -> String {
        format!("box {}", id.0)
    }

    /// The name of the output ending in `suffix`, as the design reports it.
    fn output(l: &Layout, suffix: &str) -> String {
        let o = simulate(l, 10.0, &NoDocuments, &Machine::default(), None, &Options::default());
        outputs(l, &o, &names).into_iter().map(|o| o.0).find(|n| n.ends_with(suffix)).expect("that output")
    }

    fn laser_into(block: Builtin, wavelength: f32) -> (Layout, InstanceId) {
        let mut l = Layout::default();
        l.components.push(PlacedComponent {
            kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
            pos: cell_center(0, 0),
        });
        let b = l.add_instance(BlockRef::Builtin(block), (4, 0));
        l.connect_feed(0, PortRef { instance: b, port: 0 });
        (l, b)
    }

    #[test]
    fn a_coupler_reaches_a_target_power() {
        // 1 mW in; the bar port should carry 0.25 mW: κ = 0.75.
        let (l, b) = laser_into(Builtin::Coupler { coupling: 0.5 }, 1550.0);
        let vars = [Variable { item: Item::Box(b), param: "coupling".into(), min: 0.0, max: 1.0 }];
        let objective = Objective { output: output(&l, " out1"), goal: Goal::Target(0.25) };
        let r = optimise(&l, 10.0, &NoDocuments, &Machine::default(), None, &vars, &objective, 200, &names);
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!((r.values[0] - 0.75).abs() < 1e-4, "κ {}", r.values[0]);
        assert!((r.after - 0.25).abs() < 1e-6 && (r.before - 0.5).abs() < 1e-12);
    }

    #[test]
    fn a_lossy_ring_is_darkest_at_critical_coupling() {
        // On resonance, the through port vanishes when the coupling matches
        // the round-trip loss: r = a, κ = 1 − a². 1 mm at 20 dB/cm: a = 10^(−0.1).
        let ring = Builtin::Ring { coupling: 0.1, length_um: 1000.0, n_eff: 2.4, loss_db_per_cm: 20.0, n_g: 2.4, reference_nm: 1550.0 };
        // No dispersion: resonant where m λ = n_eff L.
        let m = (2.4 * 1000.0 / 1.55_f64).round();
        let (l, b) = laser_into(ring, (2.4 * 1_000_000.0 / m) as f32);
        let vars = [Variable { item: Item::Box(b), param: "coupling".into(), min: 0.01, max: 0.9 }];
        let objective = Objective { output: output(&l, " out1"), goal: Goal::Minimise };
        let r = optimise(&l, 10.0, &NoDocuments, &Machine::default(), None, &vars, &objective, 300, &names);
        assert!(r.error.is_none(), "{:?}", r.error);
        let a = 10f64.powf(-0.1);
        let critical = 1.0 - a * a;
        assert!((r.values[0] - critical).abs() < 2e-3, "κ {} vs critical {critical}", r.values[0]);
        assert!(r.after < 1e-5 && r.after < r.before, "darkest: {} (from {})", r.after, r.before);
    }

    #[test]
    fn two_outputs_are_aimed_at_together() {
        // 1 mW in: 0.25 mW at one output and 0.75 mW at the other, at once:
        // only κ = 0.75 does both.
        let (l, b) = laser_into(Builtin::Coupler { coupling: 0.5 }, 1550.0);
        let vars = [Variable { item: Item::Box(b), param: "coupling".into(), min: 0.0, max: 1.0 }];
        let targets = [
            Target { output: output(&l, " out1"), goal: Goal::Target(0.25), band: None, weight: 1.0 },
            Target { output: output(&l, " out2"), goal: Goal::Target(0.75), band: None, weight: 1.0 },
        ];
        let r = optimise_targets(&l, 10.0, &NoDocuments, &Machine::default(), None, &vars, &targets, 200, &read_outputs(&names));
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!((r.values[0] - 0.75).abs() < 1e-4, "κ {}", r.values[0]);
        assert!((r.before[0] - 0.5).abs() < 1e-12 && (r.after[0] - 0.25).abs() < 1e-6 && (r.after[1] - 0.75).abs() < 1e-6, "{r:?}");
        assert!(r.cost_after < r.cost_before);
    }

    #[test]
    fn a_rings_notch_is_shaped_across_a_band() {
        // The through port darkened across 0.1 nm around the resonance, on
        // average: the band's simulations are counted, and it gets darker.
        let ring = Builtin::Ring { coupling: 0.02, length_um: 1000.0, n_eff: 2.4, loss_db_per_cm: 20.0, n_g: 2.4, reference_nm: 1550.0 };
        let m = (2.4 * 1000.0 / 1.55_f64).round();
        let resonance = 2.4 * 1_000_000.0 / m;
        let (l, b) = laser_into(ring, resonance as f32);
        let vars = [Variable { item: Item::Box(b), param: "coupling".into(), min: 0.01, max: 0.9 }];
        let band = Band { from_nm: resonance - 0.05, to_nm: resonance + 0.05, points: 5 };
        let targets = [Target { output: output(&l, " out1"), goal: Goal::Minimise, band: Some(band), weight: 1.0 }];
        let r = optimise_targets(&l, 10.0, &NoDocuments, &Machine::default(), None, &vars, &targets, 100, &read_outputs(&names));
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(r.after[0] < 0.5 * r.before[0], "the band's average: {} → {}", r.before[0], r.after[0]);
        assert!(r.simulations > 5 * 20, "each step simulates the band: {}", r.simulations);
        // Without a laser, a band can't be tuned.
        let mut dark = l.clone();
        dark.components.clear();
        dark.feeds.clear();
        let r = optimise_targets(&dark, 10.0, &NoDocuments, &Machine::default(), None, &vars, &targets, 10, &read_outputs(&names));
        assert!(r.error.is_some());
    }
}
