//! Monte Carlo: how a design holds up to fabrication variation. Each sample
//! is the design with every block's and part's parameters drawn around
//! their values (normal distributions), simulated in full; the outputs'
//! spread, and how many samples meet a spec, say how robust it is.

use rand::SeedableRng;
use rand::rngs::StdRng;
use rand_distr::{Distribution, Normal};

use super::sim::{Documents, Machine, Options, simulate};
use super::sweep::outputs;
use crate::panels::optics::model::{BlockRef, Builtin, InstanceId, Layout, OpticComponentKind};
use crate::panels::project::DocId;

/// How much fabrication moves each kind of parameter: standard deviations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variation {
    /// Effective index of waveguides and rings (absolute): width and
    /// thickness variation, the strongest effect on resonances.
    pub n_eff: f64,
    /// Couplings and splitting ratios, relative (0.05 = 5 %).
    pub coupling: f64,
    /// Phases of shifters and MZIs, in radians (imbalance).
    pub phase: f64,
    /// Propagation and element loss, relative.
    pub loss: f64,
}

impl Default for Variation {
    /// Typical of silicon photonics: a few thousandths of index, a few percent of coupling.
    fn default() -> Self {
        Self { n_eff: 0.002, coupling: 0.03, phase: 0.05, loss: 0.1 }
    }
}

/// One output over the samples.
#[derive(Clone, Debug, PartialEq)]
pub struct Samples {
    pub name: String,
    /// Power (mW) in each sample, in sample order.
    pub power: Vec<f64>,
    /// Power with no variation.
    pub nominal: f64,
    pub from_box: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonteCarlo {
    pub seed: u64,
    pub series: Vec<Samples>,
    pub error: Option<String>,
}

/// Summary of one output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub mean: f64,
    pub std: f64,
    pub min: f64,
    pub max: f64,
    /// 5th and 95th percentiles: nine in ten samples fall between them.
    pub p5: f64,
    pub p95: f64,
}

impl Samples {
    pub fn stats(&self) -> Stats {
        let n = self.power.len().max(1) as f64;
        let mean = self.power.iter().sum::<f64>() / n;
        let var = self.power.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
        let mut sorted = self.power.clone();
        sorted.sort_by(f64::total_cmp);
        let at = |q: f64| sorted.get(((sorted.len().saturating_sub(1)) as f64 * q).round() as usize).copied().unwrap_or(0.0);
        Stats { mean, std: var.sqrt(), min: at(0.0), max: at(1.0), p5: at(0.05), p95: at(0.95) }
    }

    /// The share of samples with power at least `min_mw` (and at most
    /// `max_mw`, if given).
    pub fn yield_of(&self, min_mw: f64, max_mw: Option<f64>) -> f64 {
        if self.power.is_empty() {
            return 0.0;
        }
        let pass = self.power.iter().filter(|&&p| p >= min_mw && max_mw.is_none_or(|m| p <= m)).count();
        pass as f64 / self.power.len() as f64
    }
}

/// A draw from a normal distribution of spread `sigma` around 0.
fn normal(rng: &mut StdRng, sigma: f64) -> f64 {
    if sigma > 0.0 { Normal::new(0.0, sigma).map_or(0.0, |d| d.sample(rng)) } else { 0.0 }
}

/// `x` varied by a relative spread `sigma`.
fn relative(rng: &mut StdRng, x: f64, sigma: f64) -> f64 {
    x * (1.0 + normal(rng, sigma))
}

/// `layout` with its parameters drawn from `v`, from `rng`.
pub fn vary(layout: &Layout, v: &Variation, rng: &mut StdRng) -> Layout {
    let mut l = layout.clone();
    for inst in &mut l.instances {
        let BlockRef::Builtin(b) = &mut inst.block else { continue };
        match b {
            Builtin::Coupler { coupling } => *coupling = relative(rng, *coupling, v.coupling).clamp(0.0, 1.0),
            Builtin::PhaseShifter { phase } => *phase += normal(rng, v.phase),
            Builtin::Attenuator { loss_db } => *loss_db = relative(rng, *loss_db, v.loss).max(0.0),
            Builtin::DelayLine { n_eff, loss_db_per_cm, .. } => {
                *n_eff += normal(rng, v.n_eff);
                *loss_db_per_cm = relative(rng, *loss_db_per_cm, v.loss).max(0.0);
            }
            Builtin::Mzi { theta, phi } => {
                *theta += normal(rng, v.phase);
                *phi += normal(rng, v.phase);
            }
            Builtin::Ring { coupling, n_eff, loss_db_per_cm, .. } | Builtin::AddDropRing { coupling, n_eff, loss_db_per_cm, .. } => {
                *coupling = relative(rng, *coupling, v.coupling).clamp(0.0, 1.0);
                *n_eff += normal(rng, v.n_eff);
                *loss_db_per_cm = relative(rng, *loss_db_per_cm, v.loss).max(0.0);
            }
            _ => {}
        }
    }
    for part in &mut l.components {
        match &mut part.kind {
            OpticComponentKind::BeamSplitter { reflectivity, transparency, .. } => {
                *reflectivity = (relative(rng, *reflectivity as f64, v.coupling) as f32).clamp(0.0, 1.0);
                *transparency = 1.0 - *reflectivity;
            }
            OpticComponentKind::PhaseShifter { phase } => *phase += normal(rng, v.phase) as f32,
            OpticComponentKind::Attenuator { transmission } => *transmission = (relative(rng, *transmission as f64, v.loss) as f32).clamp(0.0, 1.0),
            _ => {}
        }
    }
    l
}

/// Simulate `samples` varied copies of `layout`. Sample `k` draws from its
/// own stream (`seed`, `k`), so results repeat whatever the thread count.
#[allow(clippy::too_many_arguments)]
pub fn run(
    layout: &Layout,
    cell_um: f64,
    docs: &(dyn Documents + Sync),
    machine: &Machine,
    root: Option<DocId>,
    v: &Variation,
    samples: usize,
    seed: u64,
    name_of_box: &(dyn Fn(InstanceId) -> String + Sync),
) -> MonteCarlo {
    let mut mc = MonteCarlo { seed, ..MonteCarlo::default() };
    let nominal = simulate(layout, cell_um, docs, machine, root, &Options::default());
    if let Some(e) = nominal.error {
        mc.error = Some(e);
        return mc;
    }
    for (name, power, from_box) in outputs(layout, &nominal, name_of_box) {
        mc.series.push(Samples { name, power: Vec::with_capacity(samples), nominal: power, from_box });
    }
    let ks: Vec<u64> = (0..samples as u64).collect();
    let results = crate::photonics::par::map(&ks, |&k| {
        let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(k));
        let l = vary(layout, v, &mut rng);
        let o = simulate(&l, cell_um, docs, machine, root, &Options::default());
        match o.error {
            Some(e) => Err(format!("Sample {}: {e}", k + 1)),
            None => Ok(outputs(&l, &o, name_of_box)),
        }
    });
    for r in results {
        match r {
            Ok(outs) => {
                for s in &mut mc.series {
                    s.power.push(outs.iter().find(|o| o.0 == s.name).map_or(0.0, |o| o.1));
                }
            }
            Err(e) => {
                mc.error = Some(e);
                break;
            }
        }
    }
    mc
}

impl MonteCarlo {
    /// The samples as CSV: a row per sample, a column per output (mW).
    pub fn to_csv(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = std::iter::once("\"sample\"".to_owned())
            .chain(self.series.iter().map(|s| quote(&format!("{} power (mW)", s.name))))
            .collect::<Vec<_>>()
            .join(",");
        out.push('\n');
        let n = self.series.first().map_or(0, |s| s.power.len());
        for k in 0..n {
            out += &format!("{}", k + 1);
            for s in &self.series {
                out += &format!(",{}", s.power[k]);
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::circuit::sim::NoDocuments;
    use crate::panels::optics::model::{PlacedComponent, PortRef, cell_center};
    use egui::Vec2;

    fn no_names(id: InstanceId) -> String {
        format!("box {}", id.0)
    }

    fn laser_into(block: Builtin) -> Layout {
        let mut l = Layout::default();
        l.components.push(PlacedComponent {
            kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
            pos: cell_center(0, 0),
        });
        let b = l.add_instance(BlockRef::Builtin(block), (4, 0));
        l.connect_feed(0, PortRef { instance: b, port: 0 });
        l
    }

    #[test]
    fn a_couplers_spread_follows_its_coupling() {
        // Only the coupling varies, 5 %: the bar output is 1 − κ(1 + 0.05 N).
        let l = laser_into(Builtin::Coupler { coupling: 0.3 });
        let v = Variation { n_eff: 0.0, coupling: 0.05, phase: 0.0, loss: 0.0 };
        let mc = run(&l, 10.0, &NoDocuments, &Machine::default(), None, &v, 4000, 7, &no_names);
        assert!(mc.error.is_none(), "{:?}", mc.error);
        let bar = mc.series.iter().find(|s| s.name.ends_with(" out1")).unwrap();
        assert!((bar.nominal - 0.7).abs() < 1e-12);
        let st = bar.stats();
        assert!((st.mean - 0.7).abs() < 0.002, "mean {}", st.mean);
        assert!((st.std - 0.3 * 0.05).abs() < 0.001, "σ {} vs {}", st.std, 0.3 * 0.05);
        assert!(st.p5 < st.mean && st.mean < st.p95);
        // Symmetric spread: about half the samples reach the nominal power.
        let y = bar.yield_of(0.7, None);
        assert!((y - 0.5).abs() < 0.03, "yield {y}");
        // The same seed: the same samples.
        let again = run(&l, 10.0, &NoDocuments, &Machine::default(), None, &v, 4000, 7, &no_names);
        assert_eq!(again, mc);
        let other = run(&l, 10.0, &NoDocuments, &Machine::default(), None, &v, 4000, 8, &no_names);
        assert_ne!(other.series[0].power, mc.series[0].power);
        assert_eq!(mc.to_csv().lines().count(), 4001);
    }

    #[test]
    fn a_ring_on_resonance_is_far_more_sensitive_to_its_index_than_a_waveguide() {
        // At resonance, a ring's through port is a deep dip; a little index
        // variation moves the resonance and the dip fills in.
        let ring = Builtin::Ring { coupling: 0.05, length_um: 100.0, n_eff: 2.4, loss_db_per_cm: 3.0, n_g: 4.2, reference_nm: 1550.0 };
        let mut l = laser_into(ring);
        // Tune the laser onto the resonance nearest 1550 nm: the deepest dip
        // of the ring's own through port.
        let mut c = crate::photonics::Circuit::new();
        let r = c
            .add_block(&crate::photonics::blocks::AllPassRing { coupling: 0.05, length_um: 100.0, n_eff: 2.4, loss_db_per_cm: 3.0, n_g: 4.2, reference_nm: 1550.0 })
            .unwrap();
        let through = |wl: f64| c.transfer_matrix(wl, &[r.inputs[0]], &[r.outputs[0]]).unwrap()[(0, 0)].norm_sqr();
        let resonance = (0..30001).map(|k| 1547.0 + 6.0 * k as f64 / 30000.0).min_by(|a, b| through(*a).total_cmp(&through(*b))).unwrap();
        if let OpticComponentKind::Laser { wavelength, .. } = &mut l.components[0].kind {
            *wavelength = resonance as f32;
        }
        let v = Variation { n_eff: 0.002, coupling: 0.0, phase: 0.0, loss: 0.0 };
        let mc = run(&l, 10.0, &NoDocuments, &Machine::default(), None, &v, 400, 1, &no_names);
        let through = &mc.series[0];
        let st = through.stats();
        assert!(through.nominal < st.mean, "variation fills the dip: nominal {} mean {}", through.nominal, st.mean);
        let wg = laser_into(Builtin::DelayLine { length_um: 100.0, n_eff: 2.4, n_g: 4.2, reference_nm: 1550.0, loss_db_per_cm: 0.0 });
        let flat = run(&wg, 10.0, &NoDocuments, &Machine::default(), None, &v, 400, 1, &no_names);
        assert!(flat.series[0].stats().std < 1e-12, "a lossless waveguide's power doesn't care about its index");
        assert!(st.std > 0.01, "the ring's does: σ {}", st.std);
    }
}
