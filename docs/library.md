---
title: Rust library
description: Use Shine from Rust without the editor: the library of designs, components and analyses (shine::api), and the photonics engine underneath.
order: 13
---

# Rust library

Shine works without the editor at two levels:

- **`shine::api`**, the library: projects, designs, components, parts and ports, with
  simulation, sweeps, transmission, Monte Carlo, optimisation and exports, and files that
  open in the app. It is what the [Python](python.md) and [TypeScript](typescript.md)
  packages are, with the same names. Use the `core` feature, which leaves out the editor:

  ```toml
  [dependencies]
  shine = { git = "https://github.com/ash-hashtag/shine", default-features = false, features = ["core"] }
  ```

  ```rust
  use shine::api::Project;

  let project = Project::new();
  let chip = project.new_design("Ring filter");
  let laser = chip.laser(1550.0, 1.0)?;
  let detector = chip.detector();
  laser.to(&chip.ring(0.05, 100.0)?)?.to(&detector)?;
  let power = chip.simulate()?.power_mw(&detector)?;
  project.save("ring.shine")?;
  ```

- **`shine::photonics`**, the engine underneath: components and circuits as matrices,
  compiled and solved. With no features at all:

  ```toml
  [dependencies]
  shine = { git = "https://github.com/ash-hashtag/shine", default-features = false }
  ```

The rest of this page is about the engine.

## A Mach–Zehnder interferometer

```rust
use shine::photonics::*;

let mut c = Circuit::new();
let laser = c.add(Laser::new(1.0, 1550.0));
let split = c.add(DirectionalCoupler::balanced());
let heat = c.add(PhaseShifter::new(std::f64::consts::PI));
let join = c.add(DirectionalCoupler::balanced());
let pd = c.add(Detector::new());
c.connect(laser.output(0), split.input(0))?;
c.connect(split.output(0), heat.input(0))?;
c.connect(heat.output(0), join.input(0))?;
c.connect(split.output(1), join.input(1))?;
c.connect(join.output(0), pd.input(0))?;

let power = c.simulate(1550.0, &[])?.detected_power(pd); // 1.0
```

Conventions: fields are complex amplitudes with |E|² = power in mW; wavelengths in nm,
lengths in µm; a lossless coupler is `[[t, ik], [ik, t]]`.

## Compile once, run many times

`Circuit::compile(wavelength_nm)` turns a circuit into a `Program`: straight paths become
SIMD multiply-adds and feedback loops get an LU factorisation computed once. Run it with
one set of inputs (`run`) or a whole batch (`run_batch`, one column per input vector).
`Circuit::replace` swaps a part's parameters (for example a heater's phase) and keeps
the wiring.

## Blocks

`photonics::blocks` has reusable sub-circuits you add with `Circuit::add_block`:
splitter and combiner trees, all-pass and add–drop rings (`AddDropRing::resonant` picks
the length for a wavelength), interleavers, MZI switch trees, the optical DFT, a WDM
demultiplexer, and MZI meshes.

## Programming MZI meshes

`UnitaryMesh::decompose(&u)` finds MZI phases for a unitary matrix (a triangular, Reck
mesh). `SvdMesh::decompose(&m)` handles any matrix: two meshes and a row of attenuators,
`M = scale · U · diag(σ) · V†`. See `examples/matmul.rs`:

```sh
cargo run --release --no-default-features --example matmul
```

The full API is documented in the source (`cargo doc --no-default-features --open`).
