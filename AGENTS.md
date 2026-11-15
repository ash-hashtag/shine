# Working on Shine

Notes for anyone changing Shine, and for the coding assistants helping them. Shine is a
photonic circuit designer and simulator: a Rust editor (egui) for desktop and the browser
(WebAssembly), a command line, an MCP server, and Python/TypeScript packages. People use it
to build optical logic and circuits, then export them towards a real chip. Keep it simple,
correct, and fast on big designs.

[docs/development.md](docs/development.md) covers building, testing, the website and
releases. Plans for larger changes are in [design/](design/); read the relevant one before
working in that area.

## Where things are

| Path | What |
|---|---|
| `crates/shine-solver` | The circuit solver: components as S-matrices, circuits, MZI meshes (`shine::photonics`) |
| `crates/shine-format` | The file format: `proto/shine.proto` (Protocol Buffers) and its header |
| `src/panels/` | The design model, simulation (`circuit/`), the editor (`optics/`, `workspace/`), projects and files (`project/`) |
| `src/api/` | The library API the Python/TypeScript packages wrap (`bindings/`) |
| `src/cli*`, `src/mcp/` | `shine simulate/export/layer/network`, and the MCP server |
| `src/web.rs`, `src/web_folder.rs`, `web/` | The browser build and the website (Astro) |
| `docs/` | User documentation, rendered on the website; `docs/changelog.md` |
| `reference/` | Reference designs and the neural-network weights they're built from |
| `examples/` | Benchmarks and generators (`reference_design`, `digits`, `mnist`…) |

## Check your change locally

CI runs only when the version in `Cargo.toml` changes, or when started by hand
(**Actions → CI → Run workflow**). Ordinary pushes aren't tested there, so run the checks
yourself before pushing. CI lints with the latest stable Rust, whose clippy finds more than
older ones: `rustup update stable` first.

```sh
cargo test                                                    # everything, headless UI tests included
cargo +stable clippy --all-targets -- -D warnings             # CI uses the latest stable Rust
cargo +stable clippy --no-default-features --features core --all-targets -- -D warnings
cargo +stable clippy -p shine-photonics -- -D warnings        # the Python/TypeScript bindings
cargo test -p shine-solver -p shine-format                    # the crates on their own
cargo clippy --lib --target wasm32-unknown-unknown -- -D warnings   # the browser build
scripts/build-web.sh && scripts/e2e.sh                        # browser tests, for UI or web changes
```

The repository isn't `rustfmt`-formatted; match the style of the code around you rather
than reformatting files.

## Rules

- **Logging:** use `tracing` (`info!`, `warn!`…). No `println!`/`eprintln!`, except before
  the logger is set up or to report a crash.
- **Text in the UI wraps; it never truncates** with an ellipsis. Menus and panels widen
  to fit.
- **Designs open paused.** Simulation starts with Space (or the preference). Code and tests
  that wait for a simulation must start it (`simulate_active(true)`).
- **The file format** is `crates/shine-format/proto/shine.proto`. Change it by adding
  fields with new numbers; never renumber, reuse or change the type of a field. A new
  field's zero must mean "as before", because older files read it as zero. Files from
  Shine 0.1 (postcard, `SHNE`) open through frozen types in `src/panels/project/v*.rs`;
  don't edit those.
- **Saved reference designs** (`reference/*.shine`) come from generators. When a generator
  changes, regenerate them (`cargo run --release --example reference_design -- --write`,
  `--example digits -- --write`); tests fail until you do.
- **Docs change with the code.** Update the page that describes a behaviour in the same
  change, and add a line to the `Unreleased` section of `docs/changelog.md`. `cargo test
  docs` fails if a part, example, shortcut or MCP tool isn't documented. Screenshots come
  from the real app: `scripts/build-web.sh --screenshots`.
- **Speed claims need measurements.** Runs on one machine drift by 5–15%, so compare old
  and new builds interleaved (old, new, old, new). Don't compile while a benchmark runs.
  Keep heavy runs under about 8 GB of memory and report their peak.
- **Dependencies:** prefer a well-kept existing crate over writing your own. Anything the
  browser build uses must be pure Rust (it compiles to `wasm32-unknown-unknown`). The
  Python and npm packages are named `shine-photonics`.
- **External programs** (`src/panels/process.rs`) run only with the user's say-so: asked
  in the editor each session (never remembered), `--allow-processes` on the command line,
  never from the MCP server or the browser. Don't add a way around that, and don't make
  one run as a side effect of opening, saving or exporting a project.
- **Licences:** Shine is GPL-3.0-only. Only commit data whose licence allows it, and credit
  its source (Fashion-MNIST is MIT; MNIST, CC BY-SA 3.0, is not kept here). Never commit
  secrets or tokens, or print them in logs.

## Commits

One logical change per commit, in the order they were made, with every test passing at
each commit. The subject says what changed, in plain words; the body says why, and
anything a reviewer would otherwise have to work out. Releases follow
[docs/development.md](docs/development.md#continuous-integration-and-releases): bump
the version, date the changelog section, tag.
