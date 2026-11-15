//! Components solved where they're placed, as what they are to the design
//! around them, rather than built into one big circuit with it:
//!
//! - **Passive** (couplers, heaters, meshes; no signals): their matrix (see
//!   [`super::Reducer`]).
//! - **Driven**: light ports, and inside only passive parts, heaters and
//!   electronics that read no light (a modulator: its driver sets its
//!   heaters). To the light, a matrix that depends only on the signals at
//!   its inputs: solved for them.
//! - **Function**: signal ports only, and no memory (a full adder: lasers,
//!   interferometers and detectors inside, but only signals in and out). To
//!   the design, a function of its inputs: simulated on its own for them,
//!   and adding nothing to the design's light.
//!
//! Each is solved once for the inputs it gets: within a simulation, across
//! its rounds, and across simulations (by its content, parameters, inputs,
//! clock tick and how aliases are chosen), so an edit re-solves only the
//! components it reaches. Inside, the same again, all the way down.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use super::{Aliases, Documents, Machine, Options, configure, interface, passive};
use crate::panels::optics::model::{BlockRef, Builtin, InstanceId, Item, Layout, OpticComponentKind, Terminal};
use crate::panels::project::DocId;
use crate::photonics::Table;

/// How a component is simulated where it's placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Passive,
    Driven,
    Function,
    /// Built into the circuit around it.
    Flat,
}

/// Component `doc`'s shape.
pub(super) fn shape(docs: &dyn Documents, doc: DocId, memo: &mut HashMap<DocId, Shape>, stack: &mut Vec<DocId>) -> Shape {
    if let Some(&s) = memo.get(&doc) {
        return s;
    }
    if stack.contains(&doc) || stack.len() > 64 {
        return Shape::Flat;
    }
    let Some((l, _)) = docs.layout(doc) else { return Shape::Flat };
    stack.push(doc);
    let s = if passive(docs, doc, &mut HashMap::new(), &mut Vec::new()) {
        Shape::Passive
    } else {
        let iface = interface(l);
        let light = !iface.optical_in.is_empty() || !iface.optical_out.is_empty();
        if !light && !iface.signal_out.is_empty() && stateless(docs, l, memo, stack) {
            Shape::Function
        } else if light && driven(docs, l, memo, stack) {
            Shape::Driven
        } else {
            Shape::Flat
        }
    };
    stack.pop();
    memo.insert(doc, s);
    s
}

/// No memory anywhere inside: no registers, no scripts (which keep state).
fn stateless(docs: &dyn Documents, l: &Layout, memo: &mut HashMap<DocId, Shape>, stack: &mut Vec<DocId>) -> bool {
    l.instances.iter().all(|i| match &i.block {
        // A program keeps it in the loop, not kept by its inputs.
        BlockRef::Builtin(Builtin::Register { .. } | Builtin::Process { .. }) => false,
        BlockRef::Builtin(Builtin::Model(m)) => m.script().is_none(),
        BlockRef::Builtin(_) => true,
        BlockRef::Component(d) => {
            matches!(shape(docs, *d, memo, stack), Shape::Passive | Shape::Function | Shape::Driven)
                || docs.layout(*d).is_some_and(|(child, _)| {
                    stack.push(*d);
                    let s = !stack[..stack.len() - 1].contains(d) && stateless(docs, child, memo, stack);
                    stack.pop();
                    s
                })
        }
    })
}

/// Light through it, and nothing inside that makes or reads light other than
/// as a matrix: passive parts, heaters and attenuators (which signals may
/// drive), constants, and electronics that read no light.
fn driven(docs: &dyn Documents, l: &Layout, memo: &mut HashMap<DocId, Shape>, stack: &mut Vec<DocId>) -> bool {
    l.feeds.is_empty()
        && l.components.iter().all(|p| matches!(p.kind, OpticComponentKind::SignalIn | OpticComponentKind::SignalOut))
        && l.instances.iter().all(|i| match &i.block {
            BlockRef::Component(d) => matches!(shape(docs, *d, memo, stack), Shape::Passive | Shape::Driven | Shape::Function),
            BlockRef::Builtin(Builtin::FunctionCell { light_inputs, light_outputs, .. }) => *light_inputs == 0 && light_outputs.is_empty(),
            BlockRef::Builtin(Builtin::Model(m)) => m.script().is_none() && m.signal_inputs.is_empty() && m.signal_outputs.is_empty(),
            BlockRef::Builtin(b) => matches!(
                b,
                Builtin::InputPin { .. }
                    | Builtin::OutputPin { .. }
                    | Builtin::Coupler { .. }
                    | Builtin::PhaseShifter { .. }
                    | Builtin::Attenuator { .. }
                    | Builtin::DelayLine { .. }
                    | Builtin::Mzi { .. }
                    | Builtin::YSplitter
                    | Builtin::YCombiner
                    | Builtin::Crossing
                    | Builtin::Ring { .. }
                    | Builtin::AddDropRing { .. }
                    | Builtin::SplitterTree { .. }
                    | Builtin::Dft { .. }
                    | Builtin::Value { .. }
                    | Builtin::NumberInput { .. }
                    | Builtin::Display { .. }
            ),
        })
}

/// How box `inst` (in the layout at `path`) of component `doc` simulates:
/// flat when solving apart is off (netlists keep every part), or when Focus
/// looks inside it.
pub(super) fn shape_here(docs: &dyn Documents, aliases: &Aliases, reduce: bool, doc: DocId, inst: InstanceId, path: &[InstanceId], memo: &mut HashMap<DocId, Shape>) -> Shape {
    if !reduce {
        return Shape::Flat;
    }
    if let Aliases::Focus(focus) = aliases {
        let here: Vec<InstanceId> = path.iter().copied().chain([inst]).collect();
        if focus.iter().any(|f| f.len() > here.len() && f.starts_with(&here)) {
            return Shape::Flat;
        }
    }
    shape(docs, doc, memo, &mut Vec::new())
}

/// How aliases are chosen inside a box solved apart: as outside, except that
/// under Focus nothing inside is focused (Focus inside goes flat, above).
fn inner(aliases: &Aliases) -> Aliases {
    match aliases {
        Aliases::Focus(_) => Aliases::Focus(Vec::new()),
        a => a.clone(),
    }
}

thread_local! {
    /// The components being solved apart, outermost first, on this thread:
    /// one inside itself would never finish.
    static SOLVING: RefCell<Vec<DocId>> = const { RefCell::new(Vec::new()) };
}

fn guarded<T>(docs: &dyn Documents, doc: DocId, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if SOLVING.with(|s| s.borrow().contains(&doc)) {
        return Err(format!("“{}” contains itself", docs.name(doc)));
    }
    SOLVING.with(|s| s.borrow_mut().push(doc));
    let result = f();
    SOLVING.with(|s| s.borrow_mut().pop());
    result
}

/// What a solve apart is for: the component's content (with all it uses),
/// clock tick and alias choices, mixed into one number.
fn content(docs: &dyn Documents, doc: DocId, tick: u64, aliases: &Aliases, subtrees: &mut HashMap<DocId, Option<u64>>) -> Option<u64> {
    let f = super::subtree_fingerprint(docs, doc, subtrees, &mut Vec::new())?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (f, tick, aliases).hash(&mut h);
    Some(h.finish())
}

fn bits(values: &[f64]) -> Vec<u64> {
    values.iter().map(|v| v.to_bits()).collect()
}

fn param_bits(params: &[(String, f64)]) -> Vec<(String, u64)> {
    params.iter().map(|(n, v)| (n.clone(), v.to_bits())).collect()
}

/// A function's signal outputs, by content, parameters and inputs, kept
/// across simulations.
type FunctionKey = (u64, Vec<(String, u64)>, Vec<u64>);
static FUNCTIONS: Mutex<Option<HashMap<FunctionKey, Vec<f64>>>> = Mutex::new(None);
/// Kept outputs before the oldest are dropped (all of them, then: it's a
/// cache of values that are quick to work out again).
const FUNCTIONS_KEPT: usize = 200_000;

/// A component placed with parameter values and given inputs, as bits.
pub(super) type LocalKey = (DocId, Vec<(String, u64)>, Vec<u64>);
/// Within one simulation: function outputs by component, parameters and inputs.
pub(super) type Local = HashMap<LocalKey, Vec<f64>>;
/// Within one simulation: driven components' matrices, likewise.
pub(super) type Matrices = HashMap<LocalKey, Arc<Table>>;

/// The signal outputs of function component `doc` (placed with `params`)
/// for `inputs`: from what's kept, or simulated on its own.
#[allow(clippy::too_many_arguments)]
pub(super) fn call(
    docs: &dyn Documents,
    machine: &Machine,
    doc: DocId,
    params: &[(String, f64)],
    inputs: &[f64],
    local: &mut Local,
    subtrees: &mut HashMap<DocId, Option<u64>>,
) -> Result<Vec<f64>, String> {
    let near = (doc, param_bits(params), bits(inputs));
    if let Some(out) = local.get(&near) {
        return Ok(out.clone());
    }
    let aliases = inner(&machine.aliases);
    let far = content(docs, doc, machine.tick, &aliases, subtrees).map(|c| (c, near.1.clone(), near.2.clone()));
    if let Some(out) = far.as_ref().and_then(|k| FUNCTIONS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get(k).cloned()) {
        local.insert(near, out.clone());
        return Ok(out);
    }
    #[cfg(test)]
    SOLVES.with(|s| s.set(s.get() + 1));
    let out = guarded(docs, doc, || {
        let (child, cell) = docs.layout(doc).ok_or("A component used here was deleted")?;
        let configured = configure(child, params);
        let child = configured.as_ref().unwrap_or(child);
        let inner_machine = Machine { inputs: inputs.to_vec(), tick: machine.tick, aliases, ..Machine::default() };
        let o = super::simulate(child, cell, docs, &inner_machine, Some(doc), &Options::default());
        if let Some(e) = o.error {
            return Err(e);
        }
        Ok(interface(child).signal_out.iter().map(|&i| o.input(child, Terminal { item: Item::Part(i), port: 0 })).collect::<Vec<f64>>())
    })?;
    if let Some(k) = far {
        let mut kept = FUNCTIONS.lock().unwrap_or_else(|e| e.into_inner());
        let kept = kept.get_or_insert_with(HashMap::new);
        if kept.len() >= FUNCTIONS_KEPT {
            kept.clear();
        }
        kept.insert(k, out.clone());
    }
    local.insert(near, out.clone());
    Ok(out)
}

/// The matrix of driven component `doc` (placed with `params`) with
/// `inputs` on its signal inputs, at `wavelengths`: kept, or solved on its
/// own.
#[allow(clippy::too_many_arguments)]
pub(super) fn matrix(
    docs: &dyn Documents,
    machine: &Machine,
    doc: DocId,
    params: &[(String, f64)],
    inputs: &[f64],
    wavelengths: &[f64],
    local: &mut Matrices,
    subtrees: &mut HashMap<DocId, Option<u64>>,
) -> Result<Arc<Table>, String> {
    let near = (doc, param_bits(params), bits(inputs));
    if let Some(t) = local.get(&near) {
        return Ok(t.clone());
    }
    let aliases = inner(&machine.aliases);
    // In the solved components' store, the inputs as parameters of their own.
    let far = content(docs, doc, machine.tick, &aliases, subtrees).map(|c| {
        let mut params = near.1.clone();
        params.extend(near.2.iter().enumerate().map(|(k, &b)| (format!("\u{0}in{k}"), b)));
        (c, params, bits(wavelengths))
    });
    if let Some(t) = far.as_ref().and_then(super::solved::get) {
        local.insert(near, t.clone());
        return Ok(t);
    }
    let table = guarded(docs, doc, || {
        let (child, cell) = docs.layout(doc).ok_or("A component used here was deleted")?;
        let configured = configure(child, params);
        let child = configured.as_ref().unwrap_or(child);
        let inner_machine = Machine { inputs: inputs.to_vec(), tick: machine.tick, aliases, ..Machine::default() };
        let t = super::transmission(child, cell, docs, &inner_machine, Some(doc), wavelengths)?;
        Ok(Arc::new(Table { wavelengths_nm: t.wavelengths_nm, matrices: t.matrices }))
    })?;
    if let Some(k) = far {
        super::solved::keep(k, table.clone());
    }
    local.insert(near, table.clone());
    Ok(table)
}

#[cfg(test)]
thread_local! {
    /// Functions simulated on their own on this thread (tests).
    pub(super) static SOLVES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn forget() {
    SOLVES.with(|s| s.set(0));
    *FUNCTIONS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
