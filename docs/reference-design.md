---
title: Reference design
description: A 16×16 programmable photonic processor that grows with Shine, measured at sizes up to 83 000 parts, so we know what Shine can and can't do with a large design.
order: 9.7
---

# Reference design

Small examples show what each part does; they don't show whether Shine holds up when a
design gets big. The reference design does. It is a real kind of chip at a real size, it
lives in the repository (`reference/photonic-matrix-processor.shine`, and in
**File → Examples…**), and it grows with Shine: tests check it with every build, and a
benchmark measures it.

## The design: a programmable photonic processor

Programmable meshes of Mach–Zehnder interferometers are the core of optical
neural-network accelerators, optical signal processors and reconfigurable photonic chips.
This one has three levels of components:

| Level | What it is | Size |
|---|---|---|
| **MZI cell** | two 50:50 directional couplers, a heater θ between two 100 µm silicon arms, and a heater φ on the top input; θ and φ are parameters each box sets | 10 parts |
| **16×16 mesh** | 120 MZI cells in the rectangular (Clements) arrangement, 16 columns deep, with a matching waveguide wherever a mode skips a column (so every path has the same length and loss) and an output phase on each mode | 184 boxes, 288 waveguides |
| **Processor** | a 1550 nm laser, a 1×16 splitter tree, 16 MZI modulators that set the input vector, the mesh, and 16 detectors | 136 MZI cells, 1442 parts |

The mesh is programmed to the 16-point discrete Fourier transform, worked out by Shine's
rectangular decomposition (`UnitaryMesh::decompose_rectangular`). The modulators set a
cosine with two periods, so the detectors at frequency bins 0, 2 and 14 light up.

What the tests check, with every build (`tests/reference.rs`):

- The mesh's S-parameters at 1550 nm are the DFT, times the waveguides' common loss and
  phase, to 10⁻⁹.
- The detectors read |U x|², the matrix applied to the modulators' fields with the
  splitter's and waveguides' losses, to 10⁻⁹ mW, and a 4×4 processor does the same for a
  unitary with no special structure.
- The saved copy still opens and gives the same readings, as the file format moves on.

## What Shine does with it

`cargo run --release --example reference_design -- --sizes 16,64,128` builds the
processor at each size and measures it. Measured on a laptop (AMD Ryzen 5 7430U, 6 cores)
in November 2026:

| | 16×16 | 64×64 | 128×128 |
|---|---|---|---|
| MZI cells / parts opened up | 136 / 1 442 | 2 080 / 21 122 | 8 256 / 83 202 |
| Boxes / waveguides in the mesh document | 184 / 288 | 2 272 / 4 224 | 8 640 / 16 640 |
| File size; save; open | 7 kB; 0.1 ms; 0.1 ms | 96 kB; 0.5 ms; 0.6 ms | 373 kB; 1.5 ms; 1.6 ms |
| One simulation | 4 ms | 56 ms | 0.26 s |
| Wavelength sweep, 201 points | 0.15 s | 2.7 s | 11 s |
| Mesh S-parameters, 201 wavelengths | 0.07 s (32 ports) | 1.9 s (128 ports) | 10.5 s (256 ports) |
| SAX netlist; gdsfactory layout | 4 ms; 3 ms | 66 ms; 54 ms | 0.29 s; 0.25 s |
| Editor frame, idle or zooming: processor | 0.3 ms | 0.9 ms | 2.4 ms |
| Editor frame, idle or zooming: inside the mesh | 0.4 ms | 2.9 ms | 7.4 ms |
| Editor frame after an edit (re-simulates) | 4 ms | 55 ms | 260 ms |

## What it tells us

- **Looking at and navigating a large design is quick.** Even inside the 128×128 mesh,
  8 640 boxes and 16 640 waveguides, a frame takes 7 ms. (Before this design existed it
  took 160 ms: every waveguide looked its boxes up one by one, and off-screen waveguides
  were drawn. The reference design found that.)
- **Simulation scales about linearly** with the number of parts, and stays exact: the
  128×128 mesh is 83 000 parts and simulates in a quarter of a second.
- **Sweeps and S-parameters are runs, not live updates**: seconds for a large design, as
  in other circuit simulators. They use every core.
- **Limit: edits re-simulate before the next frame.** Up to about 32×32 (5 000 parts) that
  is under a frame's time; above it, each edit holds the editor for the length of one
  simulation (a quarter of a second at 128×128). Simulating in the background, so the
  editor never waits, is [on the roadmap](roadmap.md#milestone-design-at-scale).
- **Limit: the splitter tree needs a power of two inputs**, and the generic models are
  ideal (lossless couplers, a fixed group index). For a real chip, swap in your foundry
  kit's models ([Other tools and tapeout](interop.md)).

## Changing it

The design is built by code (`src/panels/optics/reference.rs`): `processor(project,
matrix, inputs)` builds one for any unitary matrix and input vector, and
`sized_project(n)` one of any power-of-two size. After changing it, save the shipped copy
again with:

```sh
cargo run --example reference_design -- --write
```
