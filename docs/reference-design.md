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

## Using it

Open **File → Examples… → 16×16 photonic matrix processor**. Light flows left to right:

1. **Laser** (*LS*, far left): 1 mW at 1550 nm, split 16 ways by the splitter tree (*1×N*).
2. **Inputs**: the column of 16 bits, each just above a modulator (*MOD*). **Click a bit**
   to switch its modulator on (1, lit) or off (0). Together they are the input vector x.
3. **The mesh** (*U16*, the tall box): multiplies x by the matrix it is programmed with,
   here the 16-point discrete Fourier transform.
4. **Outputs**: the 16 detectors (*PD*, right), each showing the power it reads and
   glowing with it. They read |U x|².

It starts with the bits 1100 1100 1100 1100, a square wave with period 4: its Fourier
transform has three peaks, so the 1st, 5th and 13th detectors light up (0.231, 0.116 and
0.116 mW) and the rest read 0. Try:

- **One bit on, the rest off**: an impulse, whose transform is flat. Every detector reads
  the same.
- **All bits on**: a constant. Only the 1st detector (zero frequency) lights.
- **1010 1010 …**: the highest frequency. Only the 9th detector lights.

Press **Tab** on the mesh to look inside (120 MZI cells, each with its θ and φ), and Tab
on an MZI cell or a modulator to see its couplers, heaters and arms. A modulator's θ (in
the inspector) also sets any amplitude between off and on. **Analysis → Sweep…** sweeps
the laser's wavelength; **File → Export for other tools…** writes the mesh's S-parameters,
SAX netlist or gdsfactory layout.

## The design: a programmable photonic processor

Programmable meshes of Mach–Zehnder interferometers are the core of optical
neural-network accelerators, optical signal processors and reconfigurable photonic chips.
This one has these components:

| Level | What it is | Size |
|---|---|---|
| **MZI cell** | two 50:50 directional couplers, a heater θ between two 100 µm silicon arms, and a heater φ on the top input; θ and φ are parameters each box sets | 10 parts |
| **16×16 mesh** | 120 MZI cells in the rectangular (Clements) arrangement, 16 columns deep, with a matching waveguide wherever a mode skips a column (so every path has the same length and loss) and an output phase on each mode | 184 boxes, 288 waveguides |
| **Modulator** | a Mach–Zehnder modulator: Y-splitter, a heater θ = π on one 100 µm arm, Y-combiner; its *on* signal adds π, so the arms add up instead of cancelling | 7 parts |
| **Processor** | a 1550 nm laser, a 1×16 splitter tree, 16 modulators each switched by a bit, the mesh, and 16 detectors | 1 410 parts |

The mesh is programmed to the 16-point discrete Fourier transform, worked out by Shine's
rectangular decomposition (`UnitaryMesh::decompose_rectangular`).

What the tests check, with every build (`tests/reference.rs`):

- The mesh's S-parameters at 1550 nm are the DFT, times the waveguides' common loss and
  phase, to 10⁻⁹.
- The detectors read |U x|², the matrix applied to the input bits with the splitter's and
  waveguides' losses, to 10⁻⁹ mW; a 4×4 processor does the same for a unitary with no
  special structure.
- Clicking a bit changes the readings as it should: one bit on gives a flat spectrum.
- The saved copy still opens and gives the same readings, as the file format moves on.

## What Shine does with it

`cargo run --release --example reference_design -- --sizes 16,64,128` builds the
processor at each size and measures it. Measured on a laptop (AMD Ryzen 5 7430U, 6 cores)
in November 2026:

| | 16×16 | 64×64 | 128×128 |
|---|---|---|---|
| MZI cells / parts opened up | 120 / 1 410 | 2 016 / 20 994 | 8 128 / 82 946 |
| Boxes / waveguides in the mesh document | 184 / 288 | 2 272 / 4 224 | 8 640 / 16 640 |
| File size; save; open | 8 kB; 0.1 ms; 0.1 ms | 96 kB; 0.4 ms; 0.7 ms | 373 kB; 1.5 ms; 1.8 ms |
| One simulation | 4 ms | 63 ms | 0.27 s |
| Wavelength sweep, 201 points | 0.16 s | 2.6 s | 11 s |
| Mesh S-parameters, 201 wavelengths | 0.06 s (32 ports) | 1.8 s (128 ports) | 10.7 s (256 ports) |
| SAX netlist; gdsfactory layout | 4 ms; 3 ms | 62 ms; 44 ms | 0.28 s; 0.26 s |
| Editor frame, idle or zooming: processor | 0.3 ms | 0.9 ms | 2.5 ms |
| Editor frame, idle or zooming: inside the mesh | 0.4 ms | 2.7 ms | 8.4 ms |
| Editor frame after clicking an input bit | 4 ms | 1.4 ms | 3.2 ms |
| Result shown after clicking a bit | same frame | 65 ms | 0.3 s |

## What it tells us

- **Looking at and navigating a large design is quick.** Even inside the 128×128 mesh,
  8 640 boxes and 16 640 waveguides, a frame takes 7 ms. (Before this design existed it
  took 160 ms: every waveguide looked its boxes up one by one, and off-screen waveguides
  were drawn. The reference design found that.)
- **Simulation scales about linearly** with the number of parts, and stays exact: the
  128×128 processor is 83 000 parts and simulates in a quarter of a second.
- **Sweeps and S-parameters are runs, not live updates**: seconds for a large design, as
  in other circuit simulators. They use every core.
- **Editing never waits for the simulation.** A design that takes longer than about 12 ms
  to simulate does so in the background (desktop app): edits take a few milliseconds and
  the new result appears when it's ready. (Before, each edit held the editor for a whole
  simulation: a quarter of a second at 128×128.)
- **Limit: opening a large design waits for its first simulation** (a quarter of a second
  at 128×128), and in the browser, which has no threads, every simulation does.
- **Limit: the splitter tree needs a power of two inputs**, and the generic models are
  ideal (lossless couplers, a fixed group index). For a real chip, swap in your foundry
  kit's models ([Other tools and tapeout](interop.md)).

## Changing it

The design is built by code (`src/panels/optics/reference.rs`): `processor(project,
matrix, bits)` builds one for any unitary matrix and input bits, and
`sized_project(n)` one of any power-of-two size. After changing it, save the shipped copy
again with:

```sh
cargo run --example reference_design -- --write
```
