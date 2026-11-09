//! Compiled, executable form of a [`Circuit`] at a single wavelength.
//!
//! Compilation turns the netlist into a flat schedule over *slots*: one slot
//! per output port, plus one per unconnected input (where external fields are
//! injected). A connected input reads its source's slot directly, so no data is
//! ever copied along a connection.
//!
//! The schedule is a sequence of [`Step`]s over the condensation of the
//! circuit graph (strongly connected components in topological order):
//! - acyclic parts become [`Row`]s, `out = source + sum(coeff * in)`, with zero
//!   coefficients pruned — straight-line multiply-adds;
//! - each feedback loop becomes a [`Step::Solve`] with an LU factorisation
//!   computed once at compile time.
//!
//! The IR is deliberately plain data so another backend (e.g. a JIT emitting
//! machine code per row) can consume [`Program::steps`] directly.
//!
//! Execution is batched: a run evaluates many input vectors at once with fields
//! laid out as `[slot][lane]` split real/imaginary arrays, so every row is a
//! SIMD multiply-add across the batch. Large batches are split into chunks and
//! run in parallel.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use faer::linalg::solvers::{PartialPivLu, Solve};
use faer::{Mat, MatRef};
use pulp::{Arch, Simd, WithSimd};

use super::circuit::{Circuit, CircuitError, InPort, NodeId, OutPort};
use super::simd::{accumulate_power, caxpy};
use super::{C64, Matrix};

/// Lanes per parallel work item.
const CHUNK_LANES: usize = 256;

/// Maps ports to slots.
#[derive(Debug)]
pub(crate) struct Layout {
    in_offset: Vec<u32>,
    out_offset: Vec<u32>,
    /// Slot read by each input port (indexed by global input index).
    in_slot: Vec<u32>,
    /// Whether each input port is unconnected and can be driven.
    in_open: Vec<bool>,
    num_slots: usize,
}

impl Layout {
    fn new(circuit: &Circuit) -> Self {
        let mut in_offset = vec![0u32];
        let mut out_offset = vec![0u32];
        for node in &circuit.nodes {
            in_offset.push(in_offset.last().unwrap() + node.sources.len() as u32);
            out_offset.push(out_offset.last().unwrap() + node.sinks.len() as u32);
        }
        let num_outputs = *out_offset.last().unwrap() as usize;
        let mut next_free = num_outputs as u32;
        let mut in_slot = Vec::with_capacity(*in_offset.last().unwrap() as usize);
        let mut in_open = Vec::with_capacity(in_slot.capacity());
        for node in &circuit.nodes {
            for source in &node.sources {
                match source {
                    Some(p) => {
                        in_slot.push(out_offset[p.node.index()] + p.index as u32);
                        in_open.push(false);
                    }
                    None => {
                        in_slot.push(next_free);
                        in_open.push(true);
                        next_free += 1;
                    }
                }
            }
        }
        Self {
            in_offset,
            out_offset,
            in_slot,
            in_open,
            num_slots: next_free as usize,
        }
    }

    fn input_index(&self, port: InPort) -> usize {
        let i = self.in_offset[port.node.index()] as usize + port.index;
        assert!(
            i < self.in_offset[port.node.index() + 1] as usize,
            "no such input {port:?}"
        );
        i
    }

    fn input_slot(&self, port: InPort) -> usize {
        self.in_slot[self.input_index(port)] as usize
    }

    fn output_slot(&self, port: OutPort) -> usize {
        let i = self.out_offset[port.node.index()] as usize + port.index;
        assert!(
            i < self.out_offset[port.node.index() + 1] as usize,
            "no such output {port:?}"
        );
        i
    }

    fn drive_slot(&self, port: InPort) -> Result<usize, CircuitError> {
        let node = port.node.index();
        if node + 1 >= self.in_offset.len()
            || port.index >= (self.in_offset[node + 1] - self.in_offset[node]) as usize
        {
            return Err(CircuitError::NoSuchInput(port));
        }
        let i = self.input_index(port);
        if !self.in_open[i] {
            return Err(CircuitError::DrivenInputIsConnected(port));
        }
        Ok(self.in_slot[i] as usize)
    }

    fn node_inputs(&self, node: NodeId) -> Range<usize> {
        self.in_offset[node.index()] as usize..self.in_offset[node.index() + 1] as usize
    }
}

/// `target = source + sum(terms)`, each term being `coeff * state[slot]`.
#[derive(Clone, Debug)]
pub struct Row {
    /// A slot for rows in [`Step::Rows`]; a right-hand-side row index for
    /// rows belonging to a [`SolveBlock`].
    pub target: u32,
    pub source: C64,
    /// Id of the source component emitting `source` (see `Component::source_id`).
    pub source_id: u32,
    pub terms: Range<u32>,
}

/// Which sources emit during a run.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sources<'a> {
    Off,
    All,
    /// Lane `l` carries only the sources whose id is `ids[l]`: one mutually
    /// coherent group per lane, all solved in one vectorised pass.
    PerLane(&'a [u32]),
}

#[derive(Clone, Copy, Debug)]
pub struct Term {
    pub slot: u32,
    pub coeff: C64,
}

/// Feedback loop: solve `G x = rhs` where `rhs` is given by `rows` and `x`
/// is written to `unknowns`.
pub struct SolveBlock {
    pub unknowns: Vec<u32>,
    pub rows: Range<u32>,
    lu: PartialPivLu<C64>,
}

#[derive(Clone, Debug)]
pub enum Step {
    Rows(Range<u32>),
    Solve(u32),
}

pub struct Program {
    wavelength_nm: f64,
    layout: Arc<Layout>,
    steps: Vec<Step>,
    rows: Vec<Row>,
    terms: Vec<Term>,
    blocks: Vec<SolveBlock>,
}

impl Program {
    pub(crate) fn compile(circuit: &Circuit, wavelength_nm: f64) -> Result<Self, CircuitError> {
        let layout = Layout::new(circuit);
        let mut program = Program {
            wavelength_nm,
            steps: Vec::new(),
            rows: Vec::new(),
            terms: Vec::new(),
            blocks: Vec::new(),
            layout: Arc::new(Layout {
                in_offset: Vec::new(),
                out_offset: Vec::new(),
                in_slot: Vec::new(),
                in_open: Vec::new(),
                num_slots: 0,
            }),
        };

        for scc in strongly_connected_components(circuit) {
            let cyclic = scc.len() > 1 || {
                let n = &circuit.nodes[scc[0]];
                n.sinks.iter().flatten().any(|s| s.node.index() == scc[0])
            };
            if cyclic {
                program.push_block(circuit, &layout, &scc)?;
            } else {
                program.push_rows(circuit, &layout, scc[0]);
            }
        }

        program.layout = Arc::new(layout);
        Ok(program)
    }

    fn push_rows(&mut self, circuit: &Circuit, layout: &Layout, node: usize) {
        let comp = circuit.nodes[node].component.as_ref();
        let s = comp.s_matrix(self.wavelength_nm);
        let emission = comp.emission(self.wavelength_nm);
        let source_id = comp.source_id();
        let id = NodeId(node as u32);
        let inputs = layout.node_inputs(id);

        let start = self.rows.len() as u32;
        for j in 0..comp.num_outputs() {
            let terms_start = self.terms.len() as u32;
            for (k, input) in inputs.clone().enumerate() {
                let coeff = s[(j, k)];
                if coeff != C64::ZERO {
                    self.terms.push(Term {
                        slot: layout.in_slot[input],
                        coeff,
                    });
                }
            }
            self.rows.push(Row {
                target: layout.output_slot(id.output(j)) as u32,
                source: emission.as_ref().map_or(C64::ZERO, |e| e[j]),
                source_id,
                terms: terms_start..self.terms.len() as u32,
            });
        }
        let end = self.rows.len() as u32;

        // Merge with the previous run of rows to keep the schedule short.
        match self.steps.last_mut() {
            Some(Step::Rows(r)) if r.end == start => r.end = end,
            _ => self.steps.push(Step::Rows(start..end)),
        }
    }

    fn push_block(
        &mut self,
        circuit: &Circuit,
        layout: &Layout,
        scc: &[usize],
    ) -> Result<(), CircuitError> {
        let mut unknowns = Vec::new();
        for &node in scc {
            let id = NodeId(node as u32);
            for j in 0..circuit.nodes[node].sinks.len() {
                unknowns.push(layout.output_slot(id.output(j)) as u32);
            }
        }
        let index: HashMap<u32, usize> =
            unknowns.iter().enumerate().map(|(i, &s)| (s, i)).collect();

        let n = unknowns.len();
        let mut g = Mat::<C64>::identity(n, n);
        let rows_start = self.rows.len() as u32;
        let mut row = 0;
        for &node in scc {
            let comp = circuit.nodes[node].component.as_ref();
            let s = comp.s_matrix(self.wavelength_nm);
            let emission = comp.emission(self.wavelength_nm);
            let source_id = comp.source_id();
            let inputs = layout.node_inputs(NodeId(node as u32));
            for j in 0..comp.num_outputs() {
                let terms_start = self.terms.len() as u32;
                for (k, input) in inputs.clone().enumerate() {
                    let coeff = s[(j, k)];
                    if coeff == C64::ZERO {
                        continue;
                    }
                    let slot = layout.in_slot[input];
                    match index.get(&slot) {
                        Some(&col) => g[(row, col)] -= coeff,
                        None => self.terms.push(Term { slot, coeff }),
                    }
                }
                self.rows.push(Row {
                    target: row as u32,
                    source: emission.as_ref().map_or(C64::ZERO, |e| e[j]),
                source_id,
                    terms: terms_start..self.terms.len() as u32,
                });
                row += 1;
            }
        }

        let lu = g.partial_piv_lu();
        let diag = lu.U().diagonal().column_vector();
        let (mut min, mut max) = (f64::INFINITY, 0.0f64);
        for i in 0..n {
            let v = diag[i].norm();
            min = min.min(v);
            max = max.max(v);
        }
        if n > 0 && min <= 1e-12 * max.max(1.0) {
            return Err(CircuitError::Singular);
        }

        self.steps.push(Step::Solve(self.blocks.len() as u32));
        self.blocks.push(SolveBlock {
            unknowns,
            rows: rows_start..self.rows.len() as u32,
            lu,
        });
        Ok(())
    }

    pub fn wavelength_nm(&self) -> f64 {
        self.wavelength_nm
    }

    pub fn num_slots(&self) -> usize {
        self.layout.num_slots
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub fn rows(&self, range: Range<u32>) -> &[Row] {
        &self.rows[range.start as usize..range.end as usize]
    }

    pub fn terms(&self, row: &Row) -> &[Term] {
        &self.terms[row.terms.start as usize..row.terms.end as usize]
    }

    pub fn block(&self, index: u32) -> &SolveBlock {
        &self.blocks[index as usize]
    }

    /// Run once with fields injected on unconnected inputs.
    pub fn run(&self, drives: &[(InPort, C64)]) -> Result<Solution, CircuitError> {
        let ports: Vec<InPort> = drives.iter().map(|d| d.0).collect();
        let fields = Mat::from_fn(drives.len(), 1, |i, _| drives[i].1);
        self.run_batch(&ports, fields.as_ref())
    }

    /// Run a batch: column `j` of `fields` holds the fields injected on
    /// `ports` for sample `j`. Sources are active in every sample.
    pub fn run_batch(
        &self,
        ports: &[InPort],
        fields: MatRef<'_, C64>,
    ) -> Result<Solution, CircuitError> {
        self.execute(ports, fields, Sources::All)
    }

    /// The light of each group of sources on its own: solution `k` holds the
    /// fields from the sources with id `sources[k]` only, nothing injected
    /// on open inputs. All groups run together, one per SIMD lane.
    pub fn run_sources(&self, sources: &[u32]) -> Result<Vec<Solution>, CircuitError> {
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let batch = self.execute(&[], Mat::<C64>::zeros(0, sources.len()).as_ref(), Sources::PerLane(sources))?;
        Ok(sources.iter().enumerate().map(|(lane, &id)| batch.lane(lane, id)).collect())
    }

    /// Linear response from `inputs` to `outputs`, with sources disabled.
    pub fn transfer_matrix(
        &self,
        inputs: &[InPort],
        outputs: &[OutPort],
    ) -> Result<Matrix, CircuitError> {
        let n = inputs.len();
        let sol = self.execute(inputs, Mat::<C64>::identity(n, n).as_ref(), Sources::Off)?;
        Ok(Mat::from_fn(outputs.len(), n, |i, j| {
            sol.output_field_at(outputs[i], j)
        }))
    }

    fn execute(
        &self,
        ports: &[InPort],
        fields: MatRef<'_, C64>,
        sources: Sources<'_>,
    ) -> Result<Solution, CircuitError> {
        assert_eq!(ports.len(), fields.nrows(), "one row of fields per port");
        let slots = ports
            .iter()
            .map(|&p| self.layout.drive_slot(p))
            .collect::<Result<Vec<_>, _>>()?;
        let lanes = fields.ncols().max(1);

        let starts: Vec<usize> = (0..lanes).step_by(CHUNK_LANES).collect();
        let chunks = super::par::map(&starts, |&start| {
            let width = CHUNK_LANES.min(lanes - start);
            let mut state = State::zeros(self.layout.num_slots, width);
            if fields.ncols() > 0 {
                for (i, &slot) in slots.iter().enumerate() {
                    let (re, im) = state.slot_mut(slot);
                    for l in 0..width {
                        let v = fields[(i, start + l)];
                        re[l] += v.re;
                        im[l] += v.im;
                    }
                }
            }
            let sources = match sources {
                Sources::PerLane(ids) => Sources::PerLane(&ids[start..start + width]),
                other => other,
            };
            Arch::new().dispatch(Execute {
                program: self,
                state: &mut state,
                sources,
            });
            state
        });

        Ok(Solution {
            layout: self.layout.clone(),
            wavelength_nm: self.wavelength_nm,
            source: None,
            lanes,
            chunks,
        })
    }
}

/// Fields for a chunk of lanes, `[slot][lane]`, split real/imaginary.
#[derive(Clone, Debug)]
struct State {
    lanes: usize,
    re: Vec<f64>,
    im: Vec<f64>,
}

impl State {
    fn zeros(slots: usize, lanes: usize) -> Self {
        Self {
            lanes,
            re: vec![0.0; slots * lanes],
            im: vec![0.0; slots * lanes],
        }
    }

    fn range(&self, slot: usize) -> Range<usize> {
        slot * self.lanes..(slot + 1) * self.lanes
    }

    fn slot(&self, slot: usize) -> (&[f64], &[f64]) {
        let r = self.range(slot);
        (&self.re[r.clone()], &self.im[r])
    }

    fn slot_mut(&mut self, slot: usize) -> (&mut [f64], &mut [f64]) {
        let r = self.range(slot);
        (&mut self.re[r.clone()], &mut self.im[r])
    }

    fn get(&self, slot: usize, lane: usize) -> C64 {
        let i = slot * self.lanes + lane;
        C64::new(self.re[i], self.im[i])
    }
}

struct Execute<'a> {
    program: &'a Program,
    state: &'a mut State,
    sources: Sources<'a>,
}

impl WithSimd for Execute<'_> {
    type Output = ();

    #[inline(always)]
    fn with_simd<S: Simd>(self, simd: S) {
        let Self {
            program,
            state,
            sources,
        } = self;
        let lanes = state.lanes;
        let mut acc_re = vec![0.0; lanes];
        let mut acc_im = vec![0.0; lanes];

        // Evaluates one row into the accumulators.
        let eval = |row: &Row, state: &State, acc_re: &mut [f64], acc_im: &mut [f64]| {
            match sources {
                Sources::All => {
                    acc_re.fill(row.source.re);
                    acc_im.fill(row.source.im);
                }
                Sources::Off => {
                    acc_re.fill(0.0);
                    acc_im.fill(0.0);
                }
                Sources::PerLane(ids) => {
                    for (l, &id) in ids.iter().enumerate() {
                        let src = if row.source_id == id { row.source } else { C64::ZERO };
                        acc_re[l] = src.re;
                        acc_im[l] = src.im;
                    }
                }
            }
            for t in program.terms(row) {
                let (xr, xi) = state.slot(t.slot as usize);
                caxpy(simd, acc_re, acc_im, xr, xi, t.coeff);
            }
        };

        for step in &program.steps {
            match step {
                Step::Rows(range) => {
                    for row in program.rows(range.clone()) {
                        eval(row, state, &mut acc_re, &mut acc_im);
                        let (re, im) = state.slot_mut(row.target as usize);
                        re.copy_from_slice(&acc_re);
                        im.copy_from_slice(&acc_im);
                    }
                }
                Step::Solve(b) => {
                    let block = program.block(*b);
                    let mut rhs = Mat::<C64>::zeros(block.unknowns.len(), lanes);
                    for row in program.rows(block.rows.clone()) {
                        eval(row, state, &mut acc_re, &mut acc_im);
                        for l in 0..lanes {
                            rhs[(row.target as usize, l)] = C64::new(acc_re[l], acc_im[l]);
                        }
                    }
                    block.lu.solve_in_place(rhs.as_mut());
                    for (i, &slot) in block.unknowns.iter().enumerate() {
                        let (re, im) = state.slot_mut(slot as usize);
                        for l in 0..lanes {
                            re[l] = rhs[(i, l)].re;
                            im[l] = rhs[(i, l)].im;
                        }
                    }
                }
            }
        }
    }
}

/// Steady-state fields on every port, for each sample of a batch.
#[derive(Clone, Debug)]
pub struct Solution {
    layout: Arc<Layout>,
    wavelength_nm: f64,
    source: Option<u32>,
    lanes: usize,
    chunks: Vec<State>,
}

impl Solution {
    pub fn wavelength_nm(&self) -> f64 {
        self.wavelength_nm
    }

    /// The source id whose light this is, from [`Circuit::simulate_sources`](super::Circuit::simulate_sources).
    pub fn source(&self) -> Option<u32> {
        self.source
    }

    pub fn batch_size(&self) -> usize {
        self.lanes
    }

    /// One sample of a batch as its own solution, tagged with `source`.
    fn lane(&self, lane: usize, source: u32) -> Solution {
        let chunk = &self.chunks[lane / CHUNK_LANES];
        let l = lane % CHUNK_LANES;
        let slots = self.layout.num_slots;
        let mut state = State::zeros(slots, 1);
        for slot in 0..slots {
            let v = chunk.get(slot, l);
            state.re[slot] = v.re;
            state.im[slot] = v.im;
        }
        Solution {
            layout: self.layout.clone(),
            wavelength_nm: self.wavelength_nm,
            source: Some(source),
            lanes: 1,
            chunks: vec![state],
        }
    }

    fn value(&self, slot: usize, lane: usize) -> C64 {
        assert!(lane < self.lanes, "lane {lane} out of range");
        self.chunks[lane / CHUNK_LANES].get(slot, lane % CHUNK_LANES)
    }

    /// Field arriving at an input port (first sample of the batch).
    pub fn input_field(&self, port: InPort) -> C64 {
        self.input_field_at(port, 0)
    }

    pub fn input_field_at(&self, port: InPort, lane: usize) -> C64 {
        self.value(self.layout.input_slot(port), lane)
    }

    /// Field leaving an output port (first sample of the batch).
    pub fn output_field(&self, port: OutPort) -> C64 {
        self.output_field_at(port, 0)
    }

    pub fn output_field_at(&self, port: OutPort, lane: usize) -> C64 {
        self.value(self.layout.output_slot(port), lane)
    }

    /// Total power arriving on all inputs of `node` (first sample).
    pub fn detected_power(&self, node: NodeId) -> f64 {
        self.detected_power_at(node, 0)
    }

    pub fn detected_power_at(&self, node: NodeId, lane: usize) -> f64 {
        self.layout
            .node_inputs(node)
            .map(|i| self.value(self.layout.in_slot[i] as usize, lane).norm_sqr())
            .sum()
    }

    /// Power arriving on all inputs of `node` for every sample, vectorised.
    pub fn detected_power_batch(&self, node: NodeId) -> Vec<f64> {
        struct Powers<'a> {
            sol: &'a Solution,
            node: NodeId,
        }
        impl WithSimd for Powers<'_> {
            type Output = Vec<f64>;
            #[inline(always)]
            fn with_simd<S: Simd>(self, simd: S) -> Vec<f64> {
                let mut out = Vec::with_capacity(self.sol.lanes);
                for chunk in &self.sol.chunks {
                    let mut acc = vec![0.0; chunk.lanes];
                    for i in self.sol.layout.node_inputs(self.node) {
                        let (re, im) = chunk.slot(self.sol.layout.in_slot[i] as usize);
                        accumulate_power(simd, &mut acc, re, im);
                    }
                    out.extend_from_slice(&acc);
                }
                out
            }
        }
        Arch::new().dispatch(Powers { sol: self, node })
    }
}

/// Tarjan's algorithm, iterative. Returns components in topological order
/// (every edge goes from an earlier component to a later one or stays inside).
fn strongly_connected_components(circuit: &Circuit) -> Vec<Vec<usize>> {
    let n = circuit.nodes.len();
    let succ = |v: usize| {
        circuit.nodes[v]
            .sinks
            .iter()
            .flatten()
            .map(|p| p.node.index())
    };

    const UNVISITED: usize = usize::MAX;
    let mut index = vec![UNVISITED; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut components = Vec::new();
    let mut next = 0;
    // (node, position in its successor list)
    let mut call: Vec<(usize, usize)> = Vec::new();

    for root in 0..n {
        if index[root] != UNVISITED {
            continue;
        }
        call.push((root, 0));
        while let Some(&(v, pos)) = call.last() {
            if pos == 0 && index[v] == UNVISITED {
                index[v] = next;
                low[v] = next;
                next += 1;
                stack.push(v);
                on_stack[v] = true;
            }
            if let Some(w) = succ(v).nth(pos) {
                call.last_mut().unwrap().1 += 1;
                if index[w] == UNVISITED {
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            call.pop();
            if let Some(&(parent, _)) = call.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == index[v] {
                let mut comp = Vec::new();
                loop {
                    let w = stack.pop().unwrap();
                    on_stack[w] = false;
                    comp.push(w);
                    if w == v {
                        break;
                    }
                }
                comp.sort_unstable();
                components.push(comp);
            }
        }
    }
    components.reverse();
    components
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photonics::*;

    const WL: f64 = 1550.0;

    /// Gain-less element that also emits: `out = 0.5 * in + 1`.
    #[derive(Debug)]
    struct LeakySource;

    impl Component for LeakySource {
        fn name(&self) -> &str {
            "LeakySource"
        }
        fn num_inputs(&self) -> usize {
            1
        }
        fn num_outputs(&self) -> usize {
            1
        }
        fn s_matrix(&self, _: f64) -> Matrix {
            Matrix::full(1, 1, C64::new(0.5, 0.0))
        }
        fn emission(&self, wl: f64) -> Option<Vec<C64>> {
            (wl == WL).then(|| vec![C64::ONE])
        }
    }

    fn feedforward() -> (Circuit, NodeId, NodeId, NodeId) {
        let mut c = Circuit::new();
        let a = c.add(Crossing);
        let b = c.add(PhaseShifter::new(0.0));
        let d = c.add(Detector::with_inputs(2));
        c.connect(a.output(0), b.input(0)).unwrap();
        c.connect(b.output(0), d.input(0)).unwrap();
        c.connect(a.output(1), d.input(1)).unwrap();
        (c, a, b, d)
    }

    #[test]
    fn scc_order_is_topological() {
        let mut c = Circuit::new();
        // Added out of order on purpose.
        let d = c.add(Detector::new());
        let loop_b = c.add(YCombiner);
        let loop_c = c.add(YSplitter);
        let a = c.add(PhaseShifter::new(0.0));
        c.connect(a.output(0), loop_b.input(0)).unwrap();
        c.connect(loop_b.output(0), loop_c.input(0)).unwrap();
        c.connect(loop_c.output(0), loop_b.input(1)).unwrap();
        c.connect(loop_c.output(1), d.input(0)).unwrap();
        let sccs = strongly_connected_components(&c);
        assert_eq!(
            sccs,
            vec![
                vec![a.index()],
                vec![loop_b.index(), loop_c.index()],
                vec![d.index()]
            ]
        );
    }

    #[test]
    fn scc_of_empty_and_disconnected() {
        assert!(strongly_connected_components(&Circuit::new()).is_empty());
        let mut c = Circuit::new();
        c.add(Crossing);
        c.add(Crossing);
        assert_eq!(strongly_connected_components(&c).len(), 2);
    }

    #[test]
    fn feedforward_compiles_to_one_row_run() {
        let (c, a, b, _) = feedforward();
        let p = c.compile(WL).unwrap();
        assert_eq!(p.wavelength_nm(), WL);
        // 3 outputs (2 crossing + 1 shifter) + 2 open crossing inputs.
        assert_eq!(p.num_slots(), 5);
        let [Step::Rows(range)] = p.steps() else {
            panic!("expected a single row run, got {:?}", p.steps());
        };
        let rows = p.rows(range.clone());
        assert_eq!(rows.len(), 3);
        // Zero coefficients are pruned: each crossing row has a single term.
        for row in rows {
            assert_eq!(p.terms(row).len(), 1);
            assert_eq!(row.source, C64::ZERO);
        }
        // The shifter's input reads the crossing's output slot directly.
        let shifter_row = rows.iter().find(|r| r.target as usize == 2).unwrap();
        assert_eq!(p.terms(shifter_row)[0].slot, 0);
        let _ = (a, b);
    }

    #[test]
    fn connected_inputs_alias_their_source() {
        let (c, a, b, d) = feedforward();
        let sol = c
            .simulate(
                WL,
                &[
                    (a.input(0), C64::new(0.3, 0.1)),
                    (a.input(1), C64::new(-1.0, 2.0)),
                ],
            )
            .unwrap();
        assert_eq!(sol.input_field(b.input(0)), sol.output_field(a.output(0)));
        assert_eq!(sol.input_field(d.input(1)), sol.output_field(a.output(1)));
        assert_eq!(sol.output_field(a.output(0)), C64::new(-1.0, 2.0));
        assert!((sol.detected_power(d) - (0.1 + 5.0)).abs() < 1e-12);
    }

    #[test]
    fn self_loop_becomes_solve_block_with_source() {
        let mut c = Circuit::new();
        let n = c.add(LeakySource);
        c.connect(n.output(0), n.input(0)).unwrap();
        let p = c.compile(WL).unwrap();
        let [Step::Solve(b)] = p.steps() else {
            panic!("expected a solve step, got {:?}", p.steps());
        };
        let block = p.block(*b);
        assert_eq!(block.unknowns, vec![0]);
        assert_eq!(p.rows(block.rows.clone())[0].source, C64::ONE);
        // b = 0.5 b + 1  =>  b = 2
        let sol = p.run(&[]).unwrap();
        assert!((sol.output_field(n.output(0)) - C64::new(2.0, 0.0)).norm() < 1e-12);
        // Without sources the loop is dark.
        assert_eq!(p.transfer_matrix(&[], &[n.output(0)]).unwrap().ncols(), 0);
        // At another wavelength nothing is emitted.
        let dark = c.simulate(WL + 1.0, &[]).unwrap();
        assert_eq!(dark.output_field(n.output(0)), C64::ZERO);
    }

    #[test]
    fn solve_block_reads_external_inputs() {
        // Ring with an attenuator; drive the bus and check against the
        // analytic all-pass response at resonance.
        let mut c = Circuit::new();
        let dc = c.add(DirectionalCoupler::new(0.5));
        let att = c.add(Attenuator::from_amplitude(0.5));
        c.connect(dc.output(1), att.input(0)).unwrap();
        c.connect(att.output(0), dc.input(1)).unwrap();
        let p = c.compile(WL).unwrap();
        assert!(matches!(p.steps(), [Step::Solve(_)]));
        let t = 0.5f64.sqrt();
        let expected = (t - 0.5) / (1.0 - t * 0.5);
        let sol = p.run(&[(dc.input(0), C64::ONE)]).unwrap();
        assert!((sol.output_field(dc.output(0)) - C64::new(expected, 0.0)).norm() < 1e-12);
    }

    #[test]
    fn rows_after_blocks_start_a_new_run() {
        let mut c = Circuit::new();
        let head = c.add(PhaseShifter::new(0.0));
        let a = c.add(YCombiner);
        let b = c.add(YSplitter);
        let after = c.add(PhaseShifter::new(0.0));
        c.connect(head.output(0), a.input(0)).unwrap();
        c.connect(a.output(0), b.input(0)).unwrap();
        c.connect(b.output(0), a.input(1)).unwrap();
        c.connect(b.output(1), after.input(0)).unwrap();
        let p = c.compile(WL).unwrap();
        assert!(matches!(
            p.steps(),
            [Step::Rows(_), Step::Solve(_), Step::Rows(_)]
        ));
    }

    #[test]
    fn drive_errors() {
        let (c, a, b, _) = feedforward();
        let p = c.compile(WL).unwrap();
        assert_eq!(
            p.run(&[(b.input(0), C64::ONE)]).err(),
            Some(CircuitError::DrivenInputIsConnected(b.input(0)))
        );
        assert_eq!(
            p.run(&[(a.input(7), C64::ONE)]).err(),
            Some(CircuitError::NoSuchInput(a.input(7)))
        );
        let ghost = NodeId(42);
        assert_eq!(
            p.run(&[(ghost.input(0), C64::ONE)]).err(),
            Some(CircuitError::NoSuchInput(ghost.input(0)))
        );
    }

    #[test]
    fn repeated_drives_superpose() {
        let (c, a, _, _) = feedforward();
        let sol = c
            .simulate(
                WL,
                &[(a.input(1), C64::ONE), (a.input(1), C64::new(0.0, 1.0))],
            )
            .unwrap();
        assert_eq!(sol.output_field(a.output(0)), C64::new(1.0, 1.0));
    }

    #[test]
    fn empty_inputs_and_empty_circuit() {
        let sol = Circuit::new().simulate(WL, &[]).unwrap();
        assert_eq!(sol.batch_size(), 1);
        assert_eq!(sol.wavelength_nm(), WL);
        let (c, a, _, d) = feedforward();
        let p = c.compile(WL).unwrap();
        // A batch with zero columns still yields one (undriven) sample.
        let sol = p
            .run_batch(&[a.input(0)], Matrix::zeros(1, 0).as_ref())
            .unwrap();
        assert_eq!(sol.batch_size(), 1);
        assert_eq!(sol.detected_power(d), 0.0);
    }

    #[test]
    fn batch_power_spans_chunks() {
        let (c, a, _, d) = feedforward();
        let p = c.compile(WL).unwrap();
        let n = 2 * CHUNK_LANES + 3;
        let fields = Matrix::from_fn(2, n, |i, j| C64::new(j as f64, i as f64));
        let sol = p
            .run_batch(&[a.input(0), a.input(1)], fields.as_ref())
            .unwrap();
        assert_eq!(sol.batch_size(), n);
        let powers = sol.detected_power_batch(d);
        assert_eq!(powers.len(), n);
        for j in [0, CHUNK_LANES - 1, CHUNK_LANES, n - 1] {
            let expected = (j * j) as f64 * 2.0 + 1.0;
            assert!((powers[j] - expected).abs() < 1e-9);
            assert!((sol.detected_power_at(d, j) - expected).abs() < 1e-9);
            assert_eq!(sol.input_field_at(a.input(0), j), fields[(0, j)]);
        }
    }

    #[test]
    #[should_panic(expected = "one row of fields per port")]
    fn run_batch_checks_shape() {
        let (c, a, _, _) = feedforward();
        let p = c.compile(WL).unwrap();
        let _ = p.run_batch(&[a.input(0)], Matrix::zeros(2, 1).as_ref());
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn lane_out_of_range_panics() {
        let (c, a, _, _) = feedforward();
        c.simulate(WL, &[]).unwrap().output_field_at(a.output(0), 1);
    }

    #[test]
    #[should_panic(expected = "no such output")]
    fn bad_output_port_panics() {
        let (c, a, _, _) = feedforward();
        c.simulate(WL, &[]).unwrap().output_field(a.output(5));
    }

    #[test]
    #[should_panic(expected = "no such input")]
    fn bad_input_port_panics() {
        let (c, a, _, _) = feedforward();
        c.simulate(WL, &[]).unwrap().input_field(a.input(5));
    }
}
