---
title: Other tools and tapeout
description: Export S-parameters (Touchstone) for circuit simulators, and a layout description that gdsfactory turns into a GDS or OASIS mask for a foundry.
order: 10.5
---

# Other tools and tapeout

Shine is circuit first: it makes a design work quickly. Two exports take a working design
to the tools the rest of the industry uses. Both are in **File → Export for other tools…**
and on the [command line](command-line.md#exporting).

| Export | File | For |
|---|---|---|
| **S-parameters** | Touchstone `.sNp` | circuit simulators (Lumerical INTERCONNECT, SAX, OptiSPICE…), scikit-rf, measurement software, and other designs as a black box |
| **Layout for gdsfactory** | `.pic.yml` | [gdsfactory](https://gdsfactory.github.io), which draws the mask (GDS or OASIS) with a foundry's process kit (PDK) |

## S-parameters (Touchstone)

How light passes between a design's light ports at each wavelength: the design's
**Input** and **Output** ports or pins, in the same order as on its box. Choose the design
and the wavelengths (1500 to 1600 nm, 401 points, unless you change them); the file is
named after the design, `.s4p` for four ports and so on.

- Ports `1…m` are the inputs and `m+1…m+n` the outputs. Light in Shine runs one way, so
  `S[output][input]` holds the transmission and every other entry is 0 (no reflections,
  no light running backwards).
- Frequencies are in Hz (`c / λ`), increasing; values are the field's real and imaginary
  parts, so phase is kept.
- Lasers inside the design are off; signals inside (phase shifters, switches) are at the
  values they settle to, as in the simulation.
- The header names every port and the wavelength range.

It needs at least one light input and output: a design lit by free-space beams has none.
Turn the part you want into a [component](components.md) with pins, and export that.

## Layout for gdsfactory: towards a chip

A **tapeout** sends a mask layout (GDS or OASIS) to a foundry. Drawing one needs the
foundry's process kit: its waveguides, bends, couplers and design rules. Shine doesn't
draw masks itself. It writes the design as a **gdsfactory YAML circuit**, and gdsfactory,
the open-source standard for photonic layout, draws the mask with whichever kit you use:

```python
import gdsfactory as gf
gf.gpdk.PDK.activate()          # or your foundry's PDK, e.g. the SiEPIC EBeam kit
gf.read.from_yaml("my-design.pic.yml").write_gds("my-design.gds")
```

The file's header repeats these lines. From there, gdsfactory's own tools add grating
couplers or edge couplers, route the heaters' electrical pads, and check the design
rules.

### What becomes what

| Shine | gdsfactory |
|---|---|
| Directional coupler | `mmi2x2` at 50:50, otherwise `coupler` (set its gap and length for the coupling in your kit) |
| MZI switch | `mzi2x2_2x2_phase_shifter` |
| Waveguide phase shifter | `straight_heater_metal` |
| Waveguide attenuator, delay line | `straight` (its length) |
| Ring resonator, add–drop ring | `ring_single`, `ring_double` (radius from its length) |
| Y-splitter, Y-combiner | `mmi1x2` (turned round to combine) |
| Crossing | `crossing` |
| Splitter tree, switch tree | a tree of `mmi1x2`, or of MZI switches |
| Input / Output pins | the chip's ports, by their names |
| Lasers wired into boxes, laser and detector blocks | the chip's ports, where light enters or leaves |
| Components used in the design | flattened into it; names start with the component's |

- **Waveguides** become routes that gdsfactory draws, with bends where needed.
- **Every waveguide end left open** becomes a port of the chip, named after its component
  and port (`ring_resonator_18_0_o2`): where a fibre coupler or another circuit attaches.
- **Placement** keeps the order and arrangement of the grid. Each column is as wide as the
  largest component in it, plus a spacing (40 µm, or **Spacing** in the export window), so
  components don't overlap.
- Shine's numbers that gdsfactory can't take directly (a coupler's coupling, a ring's
  loss, a phase) are written as comments beside each component.

**Left out**, with a note: free-space parts (they aren't on a chip), the electronics, and
the optical DFT block (build it from couplers and phase shifters). A ring shorter than
2π × 5 µm would bend more tightly than the generic kit allows, so it gets a 5 µm radius
(a longer ring, so a smaller FSR).

The generic kit stands in for a real one. Before a tapeout, check sizes, gaps and spacing
against your foundry's kit and run its design-rule check.
