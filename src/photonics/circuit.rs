//! Netlist description of an optical circuit.
//!
//! A [`Circuit`] only records components and connections. Turning it into
//! something that can be evaluated is the job of [`Circuit::compile`], which
//! lowers the netlist for one wavelength into a [`Program`].

use std::fmt;

use super::program::Program;
use super::{C64, Component, Matrix};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn input(self, index: usize) -> InPort {
        InPort { node: self, index }
    }

    pub fn output(self, index: usize) -> OutPort {
        OutPort { node: self, index }
    }
}

/// An input port of a node; light flows into the component here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InPort {
    pub node: NodeId,
    pub index: usize,
}

/// An output port of a node; light leaves the component here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutPort {
    pub node: NodeId,
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CircuitError {
    NoSuchInput(InPort),
    NoSuchOutput(OutPort),
    InputAlreadyConnected(InPort),
    OutputAlreadyConnected(OutPort),
    /// External fields can only be injected into unconnected inputs.
    DrivenInputIsConnected(InPort),
    /// A replacement component must keep the same port counts.
    PortCountMismatch(NodeId),
    PortListLengthMismatch {
        from: usize,
        to: usize,
    },
    /// A feedback loop has no unique steady state (e.g. a lossless cavity
    /// exactly on resonance).
    Singular,
}

impl fmt::Display for CircuitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchInput(p) => write!(f, "node {} has no input {}", p.node.0, p.index),
            Self::NoSuchOutput(p) => write!(f, "node {} has no output {}", p.node.0, p.index),
            Self::InputAlreadyConnected(p) => {
                write!(
                    f,
                    "input {} of node {} is already connected",
                    p.index, p.node.0
                )
            }
            Self::OutputAlreadyConnected(p) => {
                write!(
                    f,
                    "output {} of node {} is already connected",
                    p.index, p.node.0
                )
            }
            Self::DrivenInputIsConnected(p) => write!(
                f,
                "input {} of node {} is connected and cannot be driven externally",
                p.index, p.node.0
            ),
            Self::PortCountMismatch(n) => {
                write!(f, "replacement for node {} has different port counts", n.0)
            }
            Self::PortListLengthMismatch { from, to } => {
                write!(f, "cannot connect {from} outputs to {to} inputs")
            }
            Self::Singular => write!(f, "circuit has no unique steady state (lossless resonance)"),
        }
    }
}

impl std::error::Error for CircuitError {}

pub(crate) struct Node {
    pub component: Box<dyn Component>,
    /// For each input, the output feeding it.
    pub sources: Vec<Option<OutPort>>,
    /// For each output, the input it feeds.
    pub sinks: Vec<Option<InPort>>,
}

/// Ports exposed by a [`Block`] after it has been built into a circuit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ports {
    pub inputs: Vec<InPort>,
    pub outputs: Vec<OutPort>,
}

/// A reusable sub-circuit: anything that can instantiate itself into a
/// [`Circuit`] and report its boundary ports (meshes, rings, MZI trees...).
pub trait Block {
    fn build(&self, circuit: &mut Circuit) -> Result<Ports, CircuitError>;
}

#[derive(Default)]
pub struct Circuit {
    pub(crate) nodes: Vec<Node>,
}

impl fmt::Debug for Circuit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.nodes.iter().map(|n| n.component.name()))
            .finish()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn add(&mut self, component: impl Component + 'static) -> NodeId {
        self.add_boxed(Box::new(component))
    }

    pub fn add_boxed(&mut self, component: Box<dyn Component>) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            sources: vec![None; component.num_inputs()],
            sinks: vec![None; component.num_outputs()],
            component,
        });
        id
    }

    pub fn add_block(&mut self, block: &impl Block) -> Result<Ports, CircuitError> {
        block.build(self)
    }

    pub fn component(&self, id: NodeId) -> &dyn Component {
        self.nodes[id.index()].component.as_ref()
    }

    /// Swap a component for another with the same ports, keeping the wiring.
    /// Useful for parameter sweeps and optimisation loops.
    pub fn replace(
        &mut self,
        id: NodeId,
        component: impl Component + 'static,
    ) -> Result<Box<dyn Component>, CircuitError> {
        let node = &mut self.nodes[id.index()];
        if component.num_inputs() != node.sources.len()
            || component.num_outputs() != node.sinks.len()
        {
            return Err(CircuitError::PortCountMismatch(id));
        }
        Ok(std::mem::replace(&mut node.component, Box::new(component)))
    }

    pub fn connect(&mut self, from: OutPort, to: InPort) -> Result<(), CircuitError> {
        let sink = self
            .nodes
            .get(from.node.index())
            .and_then(|n| n.sinks.get(from.index))
            .ok_or(CircuitError::NoSuchOutput(from))?;
        if sink.is_some() {
            return Err(CircuitError::OutputAlreadyConnected(from));
        }
        let source = self
            .nodes
            .get(to.node.index())
            .and_then(|n| n.sources.get(to.index))
            .ok_or(CircuitError::NoSuchInput(to))?;
        if source.is_some() {
            return Err(CircuitError::InputAlreadyConnected(to));
        }
        self.nodes[from.node.index()].sinks[from.index] = Some(to);
        self.nodes[to.node.index()].sources[to.index] = Some(from);
        Ok(())
    }

    /// Connect `from[i]` to `to[i]` for every `i`.
    pub fn connect_all(&mut self, from: &[OutPort], to: &[InPort]) -> Result<(), CircuitError> {
        if from.len() != to.len() {
            return Err(CircuitError::PortListLengthMismatch {
                from: from.len(),
                to: to.len(),
            });
        }
        from.iter()
            .zip(to)
            .try_for_each(|(&f, &t)| self.connect(f, t))
    }

    pub fn source_of(&self, port: InPort) -> Option<OutPort> {
        self.nodes[port.node.index()].sources[port.index]
    }

    pub fn sink_of(&self, port: OutPort) -> Option<InPort> {
        self.nodes[port.node.index()].sinks[port.index]
    }

    pub fn open_inputs(&self) -> impl Iterator<Item = InPort> + '_ {
        self.nodes.iter().enumerate().flat_map(|(i, n)| {
            n.sources
                .iter()
                .enumerate()
                .filter(|(_, s)| s.is_none())
                .map(move |(k, _)| NodeId(i as u32).input(k))
        })
    }

    pub fn open_outputs(&self) -> impl Iterator<Item = OutPort> + '_ {
        self.nodes.iter().enumerate().flat_map(|(i, n)| {
            n.sinks
                .iter()
                .enumerate()
                .filter(|(_, s)| s.is_none())
                .map(move |(k, _)| NodeId(i as u32).output(k))
        })
    }

    /// Distinct wavelengths emitted by sources in the circuit, ascending.
    pub fn emission_wavelengths(&self) -> Vec<f64> {
        let mut wls: Vec<f64> = self
            .nodes
            .iter()
            .flat_map(|n| n.component.emission_wavelengths())
            .collect();
        wls.sort_by(f64::total_cmp);
        wls.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        wls
    }

    /// Lower the circuit to an executable [`Program`] at one wavelength.
    pub fn compile(&self, wavelength_nm: f64) -> Result<Program, CircuitError> {
        Program::compile(self, wavelength_nm)
    }

    /// Compile and run once. For repeated evaluation keep the [`Program`].
    pub fn simulate(
        &self,
        wavelength_nm: f64,
        drives: &[(InPort, C64)],
    ) -> Result<Solution, CircuitError> {
        self.compile(wavelength_nm)?.run(drives)
    }

    /// Every (wavelength, source id) pair the circuit's sources emit, sorted.
    pub fn emission_channels(&self) -> Vec<(f64, u32)> {
        let mut channels: Vec<(f64, u32)> = self
            .nodes
            .iter()
            .flat_map(|n| {
                let id = n.component.source_id();
                n.component.emission_wavelengths().into_iter().map(move |wl| (wl, id))
            })
            .collect();
        channels.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        channels.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9 && a.1 == b.1);
        channels
    }

    /// Steady state of the light from the circuit's sources: one solution per
    /// wavelength and source id. Sources sharing an id interfere; separate
    /// solutions are mutually incoherent, so their powers add.
    pub fn simulate_sources(&self) -> Result<Vec<Solution>, CircuitError> {
        // Compile once per wavelength; each source group is one cheap run.
        let channels = self.emission_channels();
        let mut wavelengths: Vec<f64> = channels.iter().map(|c| c.0).collect();
        wavelengths.dedup();
        let per_wavelength = super::par::try_map(&wavelengths, |&wl| {
            let sources: Vec<u32> = channels.iter().filter(|c| c.0 == wl).map(|c| c.1).collect();
            Program::compile(self, wl)?.run_sources(&sources)
        })?;
        Ok(per_wavelength.into_iter().flatten().collect())
    }

    /// Simulate over many wavelengths in parallel.
    pub fn sweep(
        &self,
        wavelengths_nm: &[f64],
        drives: &[(InPort, C64)],
    ) -> Result<Vec<Solution>, CircuitError> {
        super::par::try_map(wavelengths_nm, |&wl| self.simulate(wl, drives))
    }

    /// Linear response from `inputs` to `outputs` (sources excluded):
    /// entry `(i, j)` is the field on `outputs[i]` per unit field on `inputs[j]`.
    pub fn transfer_matrix(
        &self,
        wavelength_nm: f64,
        inputs: &[InPort],
        outputs: &[OutPort],
    ) -> Result<Matrix, CircuitError> {
        self.compile(wavelength_nm)?
            .transfer_matrix(inputs, outputs)
    }
}

use super::program::Solution;

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;
    use crate::photonics::*;

    /// All-pass ring: bus -> coupler -> ring waveguide -> attenuator -> coupler.
    fn ring(
        coupling: f64,
        round_trip_amplitude: f64,
        length_um: f64,
    ) -> (Circuit, InPort, OutPort) {
        let mut c = Circuit::new();
        let dc = c.add(DirectionalCoupler::new(coupling));
        let wg = c.add(Waveguide::new(length_um, 1.0));
        let loss = c.add(Attenuator::from_amplitude(round_trip_amplitude));
        c.connect(dc.output(1), wg.input(0)).unwrap();
        c.connect(wg.output(0), loss.input(0)).unwrap();
        c.connect(loss.output(0), dc.input(1)).unwrap();
        (c, dc.input(0), dc.output(0))
    }

    #[test]
    fn ring_resonator_matches_analytic() {
        let (kappa, a, length): (f64, f64, f64) = (0.1, 0.8, 15.5);
        let t = (1.0 - kappa).sqrt();
        let (c, input, output) = ring(kappa, a, length);
        for wl in [1540.0, 1548.3, 1550.0, 1555.0] {
            let sol = c.simulate(wl, &[(input, C64::new(1.0, 0.0))]).unwrap();
            let e = C64::from_polar(a, propagation_phase(length, 1.0, wl));
            let expected = (t - e) / (1.0 - t * e);
            assert!(
                (sol.output_field(output) - expected).norm() < 1e-12,
                "at {wl} nm"
            );
        }
    }

    #[test]
    fn critically_coupled_ring_extinguishes_on_resonance() {
        let kappa: f64 = 0.2;
        // 15.5 um is exactly 10 wavelengths at 1550 nm.
        let (c, input, output) = ring(kappa, (1.0 - kappa).sqrt(), 15.5);
        let sol = c.simulate(1550.0, &[(input, C64::new(1.0, 0.0))]).unwrap();
        assert!(sol.output_field(output).norm() < 1e-9);
    }

    #[test]
    fn lossless_cavity_on_resonance_is_singular() {
        let (c, _, _) = ring(0.0, 1.0, 15.5);
        assert_eq!(c.compile(1550.0).err(), Some(CircuitError::Singular));
    }

    #[test]
    fn batch_matches_individual_runs() {
        let mut c = Circuit::new();
        let mzi = c.add(Mzi::new(0.4, 1.3));
        let ps = c.add(PhaseShifter::new(0.7));
        c.connect(mzi.output(0), ps.input(0)).unwrap();
        let program = c.compile(1550.0).unwrap();
        let ports = [mzi.input(0), mzi.input(1)];
        // More lanes than one chunk, and not a multiple of the SIMD width.
        let n = 601;
        let fields = Matrix::from_fn(2, n, |i, j| C64::new(j as f64 * 0.01, i as f64 - 0.5));
        let batch = program.run_batch(&ports, fields.as_ref()).unwrap();
        let powers = batch.detected_power_batch(ps);
        for j in [0, 1, 255, 256, 600] {
            let single = program
                .run(&[(ports[0], fields[(0, j)]), (ports[1], fields[(1, j)])])
                .unwrap();
            assert!(
                (single.output_field(ps.output(0)) - batch.output_field_at(ps.output(0), j)).norm()
                    < 1e-12
            );
            assert!(
                (single.output_field(mzi.output(1)) - batch.output_field_at(mzi.output(1), j))
                    .norm()
                    < 1e-12
            );
            assert!((powers[j] - single.detected_power(ps)).abs() < 1e-12);
        }
    }

    #[test]
    fn separate_loops_lit_by_separate_sources_solve_as_one_batch() {
        // Several rings, each fed by its own independent laser: in each ring's
        // solve only its own source's lane is lit. Solving them all in one
        // batch must equal solving each source alone.
        let mut c = Circuit::new();
        let mut rings = Vec::new();
        for k in 0..6u32 {
            let laser = c.add(Laser::new(1.0 + k as f64 * 0.1, 1550.0).with_source(100 + k));
            let ring = c.add_block(&blocks::AllPassRing {
                coupling: 0.05 + 0.02 * k as f64,
                length_um: 30.0 + k as f64,
                n_eff: 2.4,
                loss_db_per_cm: 10.0,
                n_g: 4.2,
                reference_nm: 1550.0,
            }).unwrap();
            c.connect(laser.output(0), ring.inputs[0]).unwrap();
            rings.push(ring.outputs[0]);
        }
        let together = c.simulate_sources().unwrap();
        assert_eq!(together.len(), 6, "one solution per source");
        for (k, sol) in together.iter().enumerate() {
            assert_eq!(sol.source(), Some(100 + k as u32));
            // Alone: the same circuit with only this laser on.
            let program = c.compile(1550.0).unwrap();
            let alone = program.run_sources(&[100 + k as u32]).unwrap().remove(0);
            for (j, &out) in rings.iter().enumerate() {
                let (a, b) = (sol.output_field(out), alone.output_field(out));
                assert!((a - b).norm() < 1e-12, "source {k}, ring {j}: {a} vs {b}");
                assert_eq!(a == C64::ZERO, j != k, "only its own ring is lit");
            }
        }
    }

    #[test]
    fn sources_drive_and_sweep() {
        let mut c = Circuit::new();
        let red = c.add(Laser::new(2.0, 650.0));
        let green = c.add(Laser::new(1.0, 532.0).with_phase(PI));
        let comb = c.add(YCombiner);
        let det = c.add(Detector::new());
        c.connect(red.output(0), comb.input(0)).unwrap();
        c.connect(green.output(0), comb.input(1)).unwrap();
        c.connect(comb.output(0), det.input(0)).unwrap();
        let sols = c.simulate_sources().unwrap();
        assert_eq!(c.emission_wavelengths(), vec![532.0, 650.0]);
        assert!((sols[0].detected_power(det) - 0.5).abs() < 1e-12);
        assert!((sols[1].detected_power(det) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn rejects_bad_connections() {
        let mut c = Circuit::new();
        let a = c.add(PhaseShifter::new(0.0));
        let b = c.add(PhaseShifter::new(0.0));
        c.connect(a.output(0), b.input(0)).unwrap();
        assert_eq!(
            c.connect(a.output(0), a.input(0)),
            Err(CircuitError::OutputAlreadyConnected(a.output(0)))
        );
        assert_eq!(
            c.connect(b.output(1), a.input(0)),
            Err(CircuitError::NoSuchOutput(b.output(1)))
        );
        assert_eq!(
            c.simulate(1550.0, &[(b.input(0), C64::ONE)]).err(),
            Some(CircuitError::DrivenInputIsConnected(b.input(0)))
        );
        assert!(c.replace(a, Mzi::new(0.0, 0.0)).is_err());
    }
}

#[cfg(test)]
mod api_tests {
    use std::f64::consts::PI;

    use super::*;
    use crate::photonics::*;

    #[test]
    fn node_and_port_helpers() {
        let mut c = Circuit::new();
        assert!(c.is_empty());
        let a = c.add(Crossing);
        let b = c.add_boxed(Box::new(YSplitter));
        assert_eq!((c.len(), a.index(), b.index()), (2, 0, 1));
        assert_eq!(a.input(1), InPort { node: a, index: 1 });
        assert_eq!(b.output(0), OutPort { node: b, index: 0 });
        assert_eq!(c.component(b).name(), "YSplitter");
        assert_eq!(format!("{c:?}"), r#"["Crossing", "YSplitter"]"#);
    }

    #[test]
    fn connections_are_queryable() {
        let mut c = Circuit::new();
        let a = c.add(YSplitter);
        let b = c.add(YCombiner);
        c.connect(a.output(1), b.input(0)).unwrap();
        assert_eq!(c.sink_of(a.output(1)), Some(b.input(0)));
        assert_eq!(c.source_of(b.input(0)), Some(a.output(1)));
        assert_eq!(c.sink_of(a.output(0)), None);
        assert_eq!(c.source_of(b.input(1)), None);
        assert_eq!(
            c.open_inputs().collect::<Vec<_>>(),
            vec![a.input(0), b.input(1)]
        );
        assert_eq!(
            c.open_outputs().collect::<Vec<_>>(),
            vec![a.output(0), b.output(0)]
        );
    }

    #[test]
    fn connect_validates_ports() {
        let mut c = Circuit::new();
        let a = c.add(PhaseShifter::new(0.0));
        let b = c.add(PhaseShifter::new(0.0));
        let ghost = NodeId(99);
        assert_eq!(
            c.connect(a.output(0), b.input(3)),
            Err(CircuitError::NoSuchInput(b.input(3)))
        );
        assert_eq!(
            c.connect(ghost.output(0), b.input(0)),
            Err(CircuitError::NoSuchOutput(ghost.output(0)))
        );
        assert_eq!(
            c.connect(a.output(0), ghost.input(0)),
            Err(CircuitError::NoSuchInput(ghost.input(0)))
        );
        c.connect(a.output(0), b.input(0)).unwrap();
        assert_eq!(
            c.connect(b.output(0), b.input(0)),
            Err(CircuitError::InputAlreadyConnected(b.input(0)))
        );
        // A failed connect leaves no half-made link behind.
        assert_eq!(c.sink_of(b.output(0)), None);
    }

    #[test]
    fn connect_all_pairs_ports() {
        let mut c = Circuit::new();
        let a = c.add(Crossing);
        let b = c.add(Crossing);
        assert_eq!(
            c.connect_all(&[a.output(0)], &[b.input(0), b.input(1)]),
            Err(CircuitError::PortListLengthMismatch { from: 1, to: 2 })
        );
        c.connect_all(&[a.output(0), a.output(1)], &[b.input(0), b.input(1)])
            .unwrap();
        // Two crossings cancel out.
        let t = c
            .transfer_matrix(
                1550.0,
                &[a.input(0), a.input(1)],
                &[b.output(0), b.output(1)],
            )
            .unwrap();
        assert_eq!(t, Matrix::identity(2, 2));
    }

    #[test]
    fn replace_keeps_wiring() {
        let mut c = Circuit::new();
        let ps = c.add(PhaseShifter::new(0.0));
        let det = c.add(Detector::new());
        c.connect(ps.output(0), det.input(0)).unwrap();
        let old = c.replace(ps, Attenuator::from_amplitude(0.5)).unwrap();
        assert_eq!(old.name(), "PhaseShifter");
        assert_eq!(c.sink_of(ps.output(0)), Some(det.input(0)));
        let sol = c.simulate(1550.0, &[(ps.input(0), C64::ONE)]).unwrap();
        assert!((sol.detected_power(det) - 0.25).abs() < 1e-12);
        assert_eq!(
            c.replace(ps, YSplitter).err().map(|e| e.to_string()),
            Some(CircuitError::PortCountMismatch(ps).to_string())
        );
    }

    #[test]
    fn emission_wavelengths_are_sorted_and_unique() {
        let mut c = Circuit::new();
        assert!(c.emission_wavelengths().is_empty());
        assert!(c.simulate_sources().unwrap().is_empty());
        c.add(Laser::new(1.0, 1550.0));
        c.add(Laser::new(1.0, 650.0));
        c.add(Laser::new(3.0, 1550.0));
        assert_eq!(c.emission_wavelengths(), vec![650.0, 1550.0]);
    }

    #[test]
    fn independent_sources_add_power() {
        // Two equal lasers into a 50:50 coupler. Phase-locked, they steer all
        // the light to one port; independent, each port gets half of each.
        for (same_source, port0) in [(true, 0.0), (false, 1.0)] {
            let mut c = Circuit::new();
            let l0 = c.add(Laser::new(1.0, 1550.0).with_phase(-PI / 2.0));
            let l1 = c.add(Laser::new(1.0, 1550.0).with_source(if same_source { 0 } else { 7 }));
            let dc = c.add(DirectionalCoupler::balanced());
            let (d0, d1) = (c.add(Detector::new()), c.add(Detector::new()));
            c.connect(l0.output(0), dc.input(0)).unwrap();
            c.connect(l1.output(0), dc.input(1)).unwrap();
            c.connect(dc.output(0), d0.input(0)).unwrap();
            c.connect(dc.output(1), d1.input(0)).unwrap();
            let sols = c.simulate_sources().unwrap();
            assert_eq!(sols.len(), if same_source { 1 } else { 2 });
            let total = |d| sols.iter().map(|s| s.detected_power(d)).fold(0.0, |a, b| a + b);
            assert!((total(d0) - port0).abs() < 1e-12, "same source {same_source}: {}", total(d0));
            assert!((total(d0) + total(d1) - 2.0).abs() < 1e-12, "power is conserved");
        }
        let mut c = Circuit::new();
        c.add(Laser::new(1.0, 1550.0).with_source(3));
        c.add(Laser::new(1.0, 1550.0).with_source(3));
        c.add(Laser::new(1.0, 1310.0).with_source(3));
        c.add(Laser::new(1.0, 1550.0));
        assert_eq!(c.emission_channels(), vec![(1310.0, 3), (1550.0, 0), (1550.0, 3)]);
        assert_eq!(c.simulate_sources().unwrap()[0].source(), Some(3));
        assert_eq!(c.simulate(1550.0, &[]).unwrap().source(), None);
    }

    #[test]
    fn coherent_sources_interfere() {
        // Two equal lasers into a coupler: relative phase steers the output.
        for (phase, port0) in [(PI / 2.0, 0.0), (-PI / 2.0, 2.0)] {
            let mut c = Circuit::new();
            let l0 = c.add(Laser::new(1.0, 1550.0));
            let l1 = c.add(Laser::new(1.0, 1550.0).with_phase(phase));
            let dc = c.add(DirectionalCoupler::balanced());
            c.connect(l0.output(0), dc.input(0)).unwrap();
            c.connect(l1.output(0), dc.input(1)).unwrap();
            let sol = &c.simulate_sources().unwrap()[0];
            assert!((sol.output_field(dc.output(0)).norm_sqr() - port0).abs() < 1e-12);
            assert!((sol.output_field(dc.output(1)).norm_sqr() - (2.0 - port0)).abs() < 1e-12);
        }
    }

    #[test]
    fn sweep_follows_dispersion() {
        let mut c = Circuit::new();
        let wg = c.add(Waveguide::new(10.0, 1.0));
        let wls = [1000.0, 1250.0, 2000.0];
        let sols = c.sweep(&wls, &[(wg.input(0), C64::ONE)]).unwrap();
        for (sol, wl) in sols.iter().zip(wls) {
            assert_eq!(sol.wavelength_nm(), wl);
            let expected = C64::from_polar(1.0, propagation_phase(10.0, 1.0, wl));
            assert!((sol.output_field(wg.output(0)) - expected).norm() < 1e-12);
        }
    }

    #[test]
    fn transfer_matrix_excludes_sources() {
        let mut c = Circuit::new();
        let laser = c.add(Laser::new(5.0, 1550.0));
        let comb = c.add(YCombiner);
        c.connect(laser.output(0), comb.input(0)).unwrap();
        let t = c
            .transfer_matrix(1550.0, &[comb.input(1)], &[comb.output(0)])
            .unwrap();
        assert!((t[(0, 0)] - C64::new(0.5f64.sqrt(), 0.0)).norm() < 1e-12);
    }

    struct Chain(usize);

    impl Block for Chain {
        fn build(&self, c: &mut Circuit) -> Result<Ports, CircuitError> {
            let nodes: Vec<_> = (0..self.0)
                .map(|_| c.add(PhaseShifter::new(PI / 4.0)))
                .collect();
            for w in nodes.windows(2) {
                c.connect(w[0].output(0), w[1].input(0))?;
            }
            Ok(Ports {
                inputs: vec![nodes[0].input(0)],
                outputs: vec![nodes[self.0 - 1].output(0)],
            })
        }
    }

    #[test]
    fn blocks_compose() {
        let mut c = Circuit::new();
        let first = c.add_block(&Chain(3)).unwrap();
        let second = c.add_block(&Chain(5)).unwrap();
        c.connect_all(&first.outputs, &second.inputs).unwrap();
        assert_eq!(c.len(), 8);
        let sol = c.simulate(1550.0, &[(first.inputs[0], C64::ONE)]).unwrap();
        // Eight eighth-turns make a full turn.
        assert!((sol.output_field(second.outputs[0]) - C64::ONE).norm() < 1e-12);
    }

    #[test]
    fn errors_display() {
        let n = NodeId(3);
        let cases = [
            (
                CircuitError::NoSuchInput(n.input(1)),
                "node 3 has no input 1",
            ),
            (
                CircuitError::NoSuchOutput(n.output(2)),
                "node 3 has no output 2",
            ),
            (
                CircuitError::InputAlreadyConnected(n.input(0)),
                "input 0 of node 3 is already connected",
            ),
            (
                CircuitError::OutputAlreadyConnected(n.output(0)),
                "output 0 of node 3 is already connected",
            ),
            (
                CircuitError::DrivenInputIsConnected(n.input(0)),
                "input 0 of node 3 is connected and cannot be driven externally",
            ),
            (
                CircuitError::PortCountMismatch(n),
                "replacement for node 3 has different port counts",
            ),
            (
                CircuitError::PortListLengthMismatch { from: 1, to: 2 },
                "cannot connect 1 outputs to 2 inputs",
            ),
            (
                CircuitError::Singular,
                "circuit has no unique steady state (lossless resonance)",
            ),
        ];
        for (err, msg) in cases {
            assert_eq!(err.to_string(), msg);
            let _: &dyn std::error::Error = &err;
        }
    }
}
