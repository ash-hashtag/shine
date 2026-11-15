//! Aliases: making one from a component's circuit, and checking one against
//! it (design/aliases.md).

use std::sync::Arc;

use super::sim::{self, Aliases, Documents, Machine, Options};
use crate::panels::models::{ModelSource, ModelSpec};
use crate::panels::optics::model::{BlockRef, Builtin, Item, Layout, Terminal};
use crate::panels::project::{Alias, DocId, Project, layout_interface};
use crate::photonics::Matrix;

/// Most signal inputs a generated truth table covers (65 536 rows).
pub const MAX_TRUTH_TABLE_INPUTS: usize = 16;
/// Up to this many signal inputs, a check tries every combination; above,
/// [`SAMPLES`] of them.
const EXHAUSTIVE_UP_TO: usize = 10;
const SAMPLES: usize = 256;
/// Wavelengths a check compares light at, nm.
const CHECK_WAVELENGTHS: [f64; 3] = [1530.0, 1550.0, 1570.0];

/// Whether `doc` keeps state between clock ticks: registers, or scripts,
/// anywhere inside.
fn has_memory(docs: &dyn Documents, doc: DocId, depth: usize) -> bool {
    let Some((l, _)) = docs.layout(doc) else { return false };
    depth < 64
        && l.instances.iter().any(|i| match &i.block {
            BlockRef::Builtin(Builtin::Register { .. }) => true,
            BlockRef::Builtin(Builtin::Model(m)) => m.script().is_some(),
            BlockRef::Builtin(_) => false,
            BlockRef::Component(d) => has_memory(docs, *d, depth + 1),
        })
}

/// The bits of `row`, input 1 the most significant, as signal values.
fn bits(row: usize, n: usize) -> Vec<f64> {
    (0..n).map(|k| ((row >> (n - 1 - k)) & 1) as f64).collect()
}

/// The input combinations a check tries: every one, or a fixed sample.
fn combinations(n: usize) -> Vec<Vec<f64>> {
    if n <= EXHAUSTIVE_UP_TO {
        return (0..1usize << n).map(|r| bits(r, n)).collect();
    }
    // A fixed pseudo-random sample, the same every time.
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..SAMPLES)
        .map(|_| {
            (0..n)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    (x & 1) as f64
                })
                .collect()
        })
        .collect()
}

/// Component `doc`'s signal outputs with `inputs` on its signal inputs,
/// simulated in full.
fn full_outputs(project: &Project, doc: DocId, layout: &Layout, cell_um: f64, inputs: &[f64]) -> Result<Vec<f64>, String> {
    let machine = Machine { inputs: inputs.to_vec(), aliases: Aliases::Full, ..Machine::default() };
    let o = sim::simulate(layout, cell_um, project, &machine, Some(doc), &Options::default());
    if let Some(e) = o.error {
        return Err(e);
    }
    let iface = sim::interface(layout);
    Ok(iface.signal_out.iter().map(|&i| o.input(layout, Terminal { item: Item::Part(i), port: 0 })).collect())
}

/// A truth table made from component `doc`'s circuit: its signal outputs
/// for every combination of its signal inputs. For logic only (no light, no
/// memory, at most [`MAX_TRUTH_TABLE_INPUTS`] inputs); exact for inputs of 0
/// and 1, and checked as it's made.
pub fn generate(project: &Project, doc: DocId) -> Result<Alias, String> {
    let d = project.doc(doc).ok_or("the component is gone")?;
    let (layout, cell_um) = (d.layout(), d.cell_um());
    let iface = layout_interface(layout);
    if !iface.inputs.is_empty() || !iface.outputs.is_empty() {
        return Err("it has light ports: Shine makes aliases of logic for now; write a script for it".into());
    }
    if iface.signal_outputs.is_empty() {
        return Err("it has no signal outputs to stand in for".into());
    }
    let n = iface.signal_inputs.len();
    if n > MAX_TRUTH_TABLE_INPUTS {
        return Err(format!("it has {n} signal inputs; a truth table covers up to {MAX_TRUTH_TABLE_INPUTS}"));
    }
    if has_memory(project, doc, 0) {
        return Err("it keeps state (registers or scripts), which a truth table can't: write a script for it".into());
    }
    let rows = (0..1usize << n).map(|r| full_outputs(project, doc, layout, cell_um, &bits(r, n))).collect::<Result<Vec<_>, _>>()?;
    let mut model = ModelSpec::new(d.name.clone(), Vec::new(), Vec::new(), ModelSource::TruthTable { rows });
    model.signal_inputs = iface.signal_inputs;
    model.signal_outputs = iface.signal_outputs;
    let checked = check(project, doc, &model)?;
    Ok(Alias { model: Arc::new(model), on: false, generated: true, checked: Some(checked) })
}

/// How far `model` is from component `doc`'s circuit: the largest
/// difference in a signal output (over every combination of 0 and 1 on its
/// signal inputs, or a fixed sample of them), or in a field from a light
/// input to a light output (at 1530, 1550 and 1570 nm, for each of those).
pub fn check(project: &Project, doc: DocId, model: &ModelSpec) -> Result<f64, String> {
    let d = project.doc(doc).ok_or("the component is gone")?;
    let (layout, cell_um) = (d.layout(), d.cell_um());
    let iface = layout_interface(layout);
    let ports = |m: &ModelSpec| (m.inputs.len(), m.outputs.len(), m.signal_inputs.len(), m.signal_outputs.len());
    let own = (iface.inputs.len(), iface.outputs.len(), iface.signal_inputs.len(), iface.signal_outputs.len());
    if ports(model) != own {
        let (a, b, c, e) = own;
        return Err(format!(
            "its ports don't match the component's ({a} light in, {b} out, {c} signals in, {e} out; the alias has {} light in, {} out, {} signals in, {} out)",
            model.inputs.len(),
            model.outputs.len(),
            model.signal_inputs.len(),
            model.signal_outputs.len()
        ));
    }
    let mut largest: f64 = 0.0;
    for inputs in combinations(own.2) {
        if own.3 > 0 {
            let full = full_outputs(project, doc, layout, cell_um, &inputs)?;
            let alias = match &model.source {
                ModelSource::TruthTable { .. } => model.truth_table_outputs(&inputs).unwrap_or_default(),
                ModelSource::Script { .. } => {
                    let wired: Vec<Option<f64>> = inputs.iter().map(|&v| Some(v)).collect();
                    sim::script_outputs(model, &wired, None)?
                }
                ModelSource::Table { .. } => vec![0.0; own.3],
            };
            for (a, b) in full.iter().zip(&alias) {
                largest = largest.max((a - b).abs());
            }
        }
        if own.0 > 0 && own.1 > 0 {
            let machine = Machine { inputs: inputs.clone(), aliases: Aliases::Full, ..Machine::default() };
            let full = sim::transmission(layout, cell_um, project, &machine, Some(doc), &CHECK_WAVELENGTHS)?;
            for (&wl, f) in CHECK_WAVELENGTHS.iter().zip(&full.matrices) {
                let a = alias_transmission(model, wl, &inputs)?;
                if a.nrows() != f.nrows() || a.ncols() != f.ncols() {
                    return Err("its light matrix has the wrong size".into());
                }
                for r in 0..f.nrows() {
                    for c in 0..f.ncols() {
                        largest = largest.max((a[(r, c)] - f[(r, c)]).norm());
                    }
                }
            }
        }
    }
    Ok(largest)
}

/// The field from each light input to each output of `model` at `wl`, with
/// `signals` on its signal inputs.
#[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
fn alias_transmission(model: &ModelSpec, wl: f64, signals: &[f64]) -> Result<Matrix, String> {
    match &model.source {
        ModelSource::Table { .. } => Ok(model.table().at(wl)),
        #[cfg(not(target_arch = "wasm32"))]
        ModelSource::Script { code } => {
            let (engine, declared) = crate::panels::script::engine(code)?;
            let signals = signals.iter().map(|&v| Some(v)).collect();
            crate::panels::script::Scripted { name: model.name.clone(), engine, declared, signals }.transmission(wl)
        }
        #[cfg(target_arch = "wasm32")]
        ModelSource::Script { .. } => Err("scripts run in the desktop app".into()),
        ModelSource::TruthTable { .. } => Ok(Matrix::zeros(model.outputs.len(), model.inputs.len())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::optics::examples;
    use crate::panels::optics::model::SimulateAs;

    /// The 8-bit adder (100 + 55), its full adder, and the sum it shows.
    fn adder() -> (Project, DocId, DocId) {
        let mut p = Project::new();
        let top = examples::eight_bit_adder(&mut p);
        let fa = p.find("Full adder").unwrap();
        (p, top, fa)
    }

    fn sum(p: &Project, top: DocId, machine: &Machine) -> u64 {
        let layout = p.doc(top).unwrap().layout();
        let o = sim::simulate(layout, 10.0, p, machine, Some(top), &Options::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        let display = layout.instances.iter().find(|i| matches!(i.block, BlockRef::Builtin(Builtin::Display { .. }))).unwrap();
        crate::panels::optics::input_number(layout, &o, display.id, 9)
    }

    fn set_alias(p: &mut Project, doc: DocId, alias: Alias) {
        p.doc_mut(doc).unwrap().component.as_mut().unwrap().alias = Some(alias);
    }

    #[test]
    fn a_full_adder_becomes_its_truth_table_and_adds_the_same() {
        let (mut p, top, fa) = adder();
        let alias = generate(&p, fa).unwrap();
        assert!(alias.generated && !alias.on);
        assert_eq!(alias.checked, Some(0.0), "exact for 0s and 1s");
        let ModelSource::TruthTable { rows } = &alias.model.source else { panic!() };
        // a, b, carry in → sum, carry out.
        assert_eq!(rows.len(), 8);
        assert_eq!(rows[0b111], vec![1.0, 1.0]);
        assert_eq!(rows[0b011], vec![0.0, 1.0]);

        let full = sum(&p, top, &Machine::default());
        assert_eq!(full, 155);
        set_alias(&mut p, fa, Alias { on: true, ..alias });
        // Every full adder by its truth table: the same sum.
        assert_eq!(sum(&p, top, &Machine::default()), 155);
        // Full, and one box forced to its circuit: still the same.
        assert_eq!(sum(&p, top, &Machine { aliases: Aliases::Full, ..Machine::default() }), 155);
        let first = p.doc(top).unwrap().layout().instances.iter().find(|i| matches!(i.block, BlockRef::Component(_))).unwrap().id;
        p.doc_mut(top).unwrap().layout_mut().instances.iter_mut().find(|i| i.id == first).unwrap().simulate = SimulateAs::Full;
        assert_eq!(sum(&p, top, &Machine::default()), 155);
        // Focus on that one: it in full, the other seven by their aliases.
        assert_eq!(sum(&p, top, &Machine { aliases: Aliases::Focus(vec![vec![first]]), ..Machine::default() }), 155);
    }

    #[test]
    fn a_wrong_alias_shows_when_checked_and_in_what_it_simulates() {
        let (mut p, top, fa) = adder();
        // Claims sum = a and carry = 0: wrong whenever b or the carry is 1.
        let script = r#"const component = {
            name: "Lazy adder",
            signals: { inputs: ["a", "b", "cin"], outputs: ["sum", "cout"] },
            outputs(s) { return [s.a ?? 0, 0]; },
        };"#;
        let model = ModelSpec::from_script(script).unwrap();
        assert_eq!(check(&p, fa, &model).unwrap(), 1.0);
        set_alias(&mut p, fa, Alias { model: Arc::new(model), on: true, generated: false, checked: None });
        // On its developer: the sum is 100 + 0 then.
        assert_eq!(sum(&p, top, &Machine::default()), 100);
        assert_eq!(sum(&p, top, &Machine { aliases: Aliases::Full, ..Machine::default() }), 155);
        // Focus on bit 0's adder (0 + 1: sum 1): it adds; the other seven copy
        // a's bits (100 = 0b1100100): 0b1100101.
        let first = p.doc(top).unwrap().layout().instances.iter().find(|i| matches!(i.block, BlockRef::Component(_))).unwrap().id;
        assert_eq!(sum(&p, top, &Machine { aliases: Aliases::Focus(vec![vec![first]]), ..Machine::default() }), 101);
    }

    #[test]
    fn what_cant_be_a_truth_table_says_why() {
        let mut p = Project::new();
        let mzi = crate::panels::optics::reference::modulator(&mut p);
        assert!(generate(&p, mzi).unwrap_err().contains("light ports"));
        // A script with the wrong ports doesn't fit.
        let (p, _, fa) = adder();
        let model = ModelSpec::from_script(crate::panels::script::TEMPLATE).unwrap();
        assert!(check(&p, fa, &model).unwrap_err().contains("ports don't match"));
    }

    #[test]
    fn a_light_alias_is_checked_on_its_light() {
        let mut p = Project::new();
        let modulator = crate::panels::optics::reference::modulator(&mut p);
        // A lossless pass-through: the real modulator has arms with loss
        // and passes sin(πv/2) of the field, so they differ.
        let script = r#"const component = {
            name: "Ideal",
            light: { inputs: ["in"], outputs: ["out"] },
            signals: { inputs: ["v"], outputs: [] },
            transmission(wl, s) { return [[1]]; },
        };"#;
        let model = ModelSpec::from_script(script).unwrap();
        let d = check(&p, modulator, &model).unwrap();
        assert!(d > 0.5, "{d}");
    }
}
