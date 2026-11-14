# Shine as crates (0.2.0)

Shine is one crate of about 40 000 lines whose parts are kept apart by Cargo features
(`core`, `gui`, `logging`). 0.2.0 splits it into a workspace of crates, each with one job,
its own tests and its own dependencies, so that:

- the solver, the design model and the editor can be worked on, tested and versioned on
  their own (a change to the canvas doesn't rebuild the solver, and the solver's tests run
  in seconds);
- the library and the Python/TypeScript packages depend on what they use, not on an
  editor behind a feature flag;
- the boundaries the code already has become ones the compiler checks.

## The crates

| Crate | What it is | Depends on |
|---|---|---|
| `shine-solver` | The frequency-domain circuit solver: components and their S-matrices, circuits, compiled programs (SIMD, threads), MZI meshes and their decompositions. `crates/shine-solver` (was `src/photonics`); `shine::photonics` re-exports it. | faer, pulp, rayon, num-complex |
| `shine-model` | Designs and files: layouts (parts, blocks, waveguides, signals), components, projects, the file format and its upgrades, S-parameter models. Today `panels::{optics::model, schematic::model, project, file_format, models, names, units}`. | shine-solver, serde, postcard |
| `shine-sim` | Simulating designs: the mixed optics–electronics simulator, sweeps, Monte Carlo, optimisation, readings, generators (matrices as light, the reference designs), and other tools' formats (Touchstone, SAX, gdsfactory). Today `panels::{circuit, interop, optics::{layers, reference, digits}}`. | shine-model, shine-solver, rquickjs (scripts), argmin, rust-rf |
| `shine-api` | The library: projects, designs, parts and ports by name, analysis. Today `src/api`. | shine-sim |
| `shine` | The app: the editor (egui), the workspace, the command line, the MCP server, the binaries. | shine-api, egui/eframe |
| `shine-photonics` | The Python and TypeScript packages (BoltFFI). Today `bindings`. | shine-api |

Each crate's public API is what the next one up uses. `shine` re-exports the others
(`shine::photonics` = `shine_solver`, …) through 0.2.x, so code written against 0.1 keeps
compiling while it moves.

## What has to come apart

- **A document's content and its editor.** `DocumentState` is the editor panel
  (`OpticsPanel`) in the app and a plain struct without it. `shine-model` keeps the
  content (layout, cell size, machine); the app keeps an editor per open document, next
  to the project rather than inside it. This is the biggest change, and it also does the
  roadmap's "documents shared copy-on-write between the editor, its undo and background
  simulations".
- **Files and the gui.** `project` has a few `cfg(feature = "gui")` items (thumbnails,
  views in the session); those move to the app.
- **Examples.** The examples (`optics::examples`, chips) build designs with the model and
  are shown by the editor: the builders go to `shine-sim`, the gallery stays in the app.

## Order

1. `shine-solver` (no dependencies on the rest: it moves as it is).
2. `shine-model`, separating document content from the editor.
3. `shine-sim`.
4. `shine-api`; `shine-photonics` depends on it instead of on `shine`.
5. `shine` is what remains: the app.

Each step keeps every test passing and is released only when the whole of 0.2.0 is.
