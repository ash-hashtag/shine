---
title: Parts reference
description: Every part and block in the library, with its ports, parameters and behaviour.
order: 3
---

# Parts reference

Select a part to edit its parameters in the inspector. Hover any part on the grid for a
short description and its current readings.

Ports are named so you can tell them apart: **light ports** are round (left edge = in,
right edge = out); **signal terminals** are square (top edge = in, bottom edge = out).

## Light & detection

| Part | What it does | Signal terminals |
|---|---|---|
| **Laser** | Source with power (in mW or dBm; 1 mW by default), phase, wavelength (with presets from violet to the telecom C band), a *source* and an optional colour. Points right; R rotates it in 90° steps. *Own* source (the default): its light never interferes with other lasers, as with real separate lasers. *Shared A–H*: lasers on the same letter are phase-locked, like light split from one laser, and interfere; the letter shows next to the laser. | in: *amplitude*, multiplies the amplitude (0 switches it off, 1 leaves it on) |
| **Detector** | Photodiode: absorbs light from any direction and reads its power in mW or dBm. As a bit (comparator) it outputs 1 at or above its threshold in mW. | out: *power* (mW) |
| **Threshold detector** | A detector whose *bit* option is on: outputs 1 when the power reaches the threshold, else 0. Any detector can be switched between the two in the inspector. | out: *power* (as 0/1) |

## Free-space optics

| Part | What it does | Signal terminals |
|---|---|---|
| **Beam Splitter** | Partially reflecting mirror; reflectivity sets the split. Rotates in 45° steps. | — |
| **Mirror** | Reflects light; the dark side absorbs. Rotates in 45° steps. | — |
| **Phase Shifter** | Adds a phase to light passing through. | in: *phase*, adds π per unit (1 flips an interferometer) |
| **Attenuator** | Passes a fraction of the power (transmission 0–1); the inspector shows it as a loss in dB. | in: *transmission*, multiplies it |

Older projects can contain a *Lens*; it is no longer in the library because a ray-on-grid
model can't focus, so it only passes light through.

## Electronics & I/O

| Block | What it does | Terminals |
|---|---|---|
| **Number input** | A number you set (1–16 bits), sent out as bits, most significant first. Click a lamp to toggle that bit. | out: b(n-1)…b0 |
| **Display** | Shows the bits on its inputs, as lamps and as a decimal number. | in: b(n-1)…b0 |
| **Register** | Memory. On each clock tick it stores its *d* inputs when *write* is high or unconnected, and holds them on its *q* outputs. | in: d…, write; out: q… |
| **Function cell** | Computes outputs from expressions over its inputs. Can read received light and emit light. See [Electronics](electronics.md#function-cells). | in: in1…; out: out1…; light in: p1…; light out: light1… |

## Example components

Ready-made [components](components.md) built from the parts above. Placing one adds a
copy to your project that you can open with Tab and change.

| Component | Ports |
|---|---|
| **Full adder** | signals in a, b, cin; out sum, cout. A teaching model: see [Examples](examples.md#8-bit-photonic-adder). |
| **Mach–Zehnder interferometer** | light in1; out out1, out2. The phase shifter inside picks the output. |
| **Fabry–Pérot filter** | light in, out. Passes only resonant wavelengths. |
| **1→4 splitter** | light in; four outputs with a quarter of the power each. |

## My components

Every layout you save as a component appears here. See [Components](components.md).

## Waveguide blocks

On-chip building blocks, drawn as boxes and connected with waveguides.

| Block | Light ports | Parameters |
|---|---|---|
| **Directional coupler** | 2 in, 2 out (bar and cross) | coupling (power to the cross port) |
| **Phase shifter** | 1 in, 1 out | phase |
| **Attenuator** | 1 in, 1 out | loss in dB |
| **Delay line** | 1 in, 1 out | length (µm), effective index; phase depends on wavelength |
| **MZI switch** | 2 in, 2 out | θ (internal: π = bar, 0 = cross), φ (input phase) |
| **Y-splitter** | 1 in, 2 out | — (equal split) |
| **Y-combiner** | 2 in, 1 out | — (only the in-phase part is guided) |
| **Crossing** | 2 in, 2 out | — (no coupling) |
| **Ring resonator** | in → through | coupling, length, effective index, loss (dB/cm) |
| **Add–drop ring** | in, add → through, drop | coupling, length, effective index, loss; resonant wavelengths go from in to drop (and from add to through) |
| **Splitter tree** | 1 in, 2^levels out | levels |
| **Switch tree** | 1 in, 2^levels out | levels, address (which output gets the light) |
| **Optical DFT** | n in, n out | modes; an MZI mesh computing the discrete Fourier transform |

A ring of length L and effective index n resonates at wavelengths λ where n·L = m·λ for a
whole number m. The [WDM link](examples.md#4-channel-wdm-optical-link) uses
L = 20·λ / 2.4 to put each ring on its own channel.

## Component ports

Used inside a layout to give it a boundary when it becomes a [component](components.md).

| Part | Becomes on the box |
|---|---|
| **Input port** | a light input on the left edge (ordered top to bottom) |
| **Output port** | a light output on the right edge |
| **Signal in** | a signal terminal on the top edge (ordered top to bottom, then left to right). In the layout's own tab, click it to switch it between 0 and 1. |
| **Signal out** | a signal terminal on the bottom edge |

Name a port in the inspector; the name shows on the box.
