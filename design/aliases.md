# Aliases: a component's shortcut for simulating

A design of a hundred components is slow to simulate and hard to debug all at once. An
**alias** is a component's stand-in: a compact model with the same ports that the simulator
can use instead of the component's full circuit. With aliases you can

- simulate a big system quickly, each component by its alias;
- test one component inside the whole system: it simulates in full, and every other
  component by its alias (**Focus**), so you watch it work among the rest;
- switch back to the full simulation at any time.

## What an alias is

A model with the component's ports (light inputs and outputs, signal inputs and outputs),
kept with the component, in one of these forms:

| Form | Made by | What it covers | Exact? |
|---|---|---|---|
| Truth table | Shine, from the component's circuit (**Generate**) | digital logic: signals only, no light, no memory; up to 16 inputs | yes, for inputs of 0 and 1 |
| Script (JavaScript) | the developer | anything: light (a transmission per wavelength, driven by signals), signal outputs, state on clock ticks | as written |
| S-parameter table | imported (Touchstone), like a model block | passive light | as measured |

Passive optical components need no alias: Shine already solves each into its exact matrix
(see "Passive components solved once" in the roadmap). Optical components driven by signals
(modulators, switches) get scripts for now; generating them (a matrix per drive level,
interpolated) comes later.

**Check** compares an alias with the full circuit: every input combination for logic (or
up to 256 sampled ones), and for light, the transmission at a few wavelengths for each. It
reports the largest difference. A generated alias is exact by construction (and checked
when made); one a developer writes is theirs to keep right, and Check is how.

## Choosing full or alias

Three levels, the narrowest winning:

1. **The design** (Simulation menu, and `--aliases full|set|focus` on the command line):
   - **Full**: every component in full; aliases ignored.
   - **As set** (the default): each component as its alias switch says (off unless turned on),
     and each box as it says.
   - **Focus on the selection**: the selected boxes simulate in full, and so does
     everything inside them; every other component with an alias uses it.
2. **The component**: **Use the alias** (off by default).
3. **The box**: *as the component says*, *full*, or *alias*.

Aliased boxes are marked on the canvas (≈ in a corner), so it's always visible which parts
of a design are shortcuts.

## How

- `ComponentMeta.alias: Option<Alias>` (`Alias { model: Arc<ModelSpec>, on, generated,
  checked: Option<f64> }`); `Instance.simulate: SimulateAs { AsSet, Full, Alias }`;
  `Machine.aliases: Aliases { AsSet, Full, Focus(Vec<Path>) }`. In the file format:
  `ComponentInfo.alias`, `Instance.simulate`, `Model.truth_table`.
- The simulator's three walkers (light, electronics, clock) each ask, at a box placing a
  component, for the alias in effect there, and treat the box as a model block of it.
- Solved passive components are cached under a key that includes the alias mode; under
  Focus, components with aliases inside aren't cached (their aliasing depends on where they
  are).

## Later

- Generated aliases for optical components driven by signals.
- Aliases as expressions (they run in the browser, which has no script engine).
- Compiling logic into one function (roadmap: "Logic solved like light"), as a generated
  alias for logic too big for a truth table.
