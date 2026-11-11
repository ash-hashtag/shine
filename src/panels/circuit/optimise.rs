//! Optimisation: find the parameter values (couplings, lengths, phases…)
//! that make an output reach a goal, by Nelder–Mead (from `argmin`), which
//! needs no derivatives: each step is a full simulation. It finds a local
//! optimum, starting from the design's current values.

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
    objective: &'a Objective,
    names: &'a (dyn Fn(InstanceId) -> String + Sync),
    count: std::sync::atomic::AtomicUsize,
}

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
        outputs(&l, &o, self.names)
            .into_iter()
            .find(|o| o.0 == self.objective.output)
            .map(|o| o.1)
            .ok_or_else(|| format!("no output “{}”", self.objective.output))
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
        let cost = match this.objective.goal {
            Goal::Maximise => -p,
            Goal::Minimise => p,
            Goal::Target(t) => (p - t).powi(2),
        };
        Ok(cost + 1e3 * outside)
    }
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
    let problem = Problem { layout, cell_um, docs, machine, root, vars, objective, names, count: Default::default() };
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
    use egui::Vec2;

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
}
