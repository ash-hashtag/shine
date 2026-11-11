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
| **Laser** | Source with power (in mW or dBm; 1 mW by default), phase, wavelength (with presets from violet to the telecom C band), a *source* and an optional colour. Points right; R rotates it in 90° steps. Unwired, its light leaves as a beam the way it faces; to feed a box, drag from its light output (the round port on the side it faces) to the box's light input. *Own* source (the default): its light never interferes with other lasers, as with real separate lasers. *Shared A–H*: lasers on the same letter are phase-locked, like light split from one laser, and interfere; the letter shows next to the laser. | in: *amplitude*, multiplies the amplitude (0 switches it off, 1 leaves it on) |
| **Detector** | Photodiode: absorbs light from any direction and reads its power in mW or dBm. As a bit (comparator) it outputs 1 at or above its threshold in mW. | out: *power* (mW) |
| **Threshold detector** | A detector whose *bit* option is on: outputs 1 when the power reaches the threshold, else 0. Any detector can be switched between the two in the inspector. | out: *power* (as 0/1) |

## Free-space optics

| Part | What it does | Signal terminals |
|---|---|---|
| **Beam Splitter** | Partially reflecting mirror; reflectivity sets the split. Rotates in 45° steps. | — |
| **Mirror** | Reflects light; the dark side absorbs. Rotates in 45° steps. | — |
| **Phase Shifter** | Adds a phase to light passing through. | in: *phase*, adds π per unit (1 flips an interferometer) |
| **Attenuator** | Passes a fraction of the power (transmission 0–1) of a beam; the inspector shows it as a loss in dB. For light in a waveguide, use the *Waveguide attenuator* block. | in: *transmission*, multiplies it (0 blocks, 1 as set) |

Older projects can contain a *Lens*; it is no longer in the library because a ray-on-grid
model can't focus, so it only passes light through.

## Electronics & I/O

| Block | What it does | Terminals |
|---|---|---|
| **Number input** | A number you set (1–16 bits), sent out as bits, most significant first. Click a lamp to toggle that bit. | out: b(n-1)…b0 |
| **Value input** | A value you set, usually 0 to 1, sent out as one signal: drives a heater or modulator, or sets an attenuator. Click along it to set it. | out: v |
| **Display** | Shows the bits on its inputs, as lamps and as a decimal number. | in: b(n-1)…b0 |
| **Register** | Memory. On each clock tick it stores its *d* inputs when *write* is high or unconnected, and holds them on its *q* outputs. | in: d…, write; out: q… |
| **Function cell** | Computes outputs from expressions over its inputs. Can read received light and emit light. See [Electronics](electronics.md#function-cells). | in: in1…; out: out1…; light in: p1…; light out: light1… |

## Example components

Ready-made [components](components.md) built from the parts above, in the **Assets** tab's
*Examples* folder. Placing one adds a copy to your project that you can open with Tab and
change.

| Component | Ports |
|---|---|
| **Full adder** | signals in a, b, cin; out sum, cout. A teaching model: see [Examples](examples.md#8-bit-photonic-adder). |
| **Mach–Zehnder interferometer** | light in1; out out1, out2. The phase shifter inside picks the output. |
| **Fabry–Pérot filter** | light in, out. Passes only resonant wavelengths. |
| **1→4 splitter** | light in; four outputs with a quarter of the power each. |

## Your components

Every layout you save as a component appears under **Assets → This project**, and the
ones you keep in the [asset library](components.md#the-asset-library) in its folders,
for every project.

## Waveguide blocks

On-chip building blocks, drawn as boxes and connected with waveguides.

| Block | Light ports | Parameters |
|---|---|---|
| **Directional coupler** | 2 in, 2 out (bar and cross) | coupling (power to the cross port) |
| **Waveguide phase shifter** | 1 in, 1 out | phase; signal *phase* on top adds π per unit (a modulator) |
| **Waveguide attenuator** | 1 in, 1 out | loss in dB; signal *transmission* on top multiplies how much passes (0 blocks, 1 as set) |
| **Delay line** | 1 in, 1 out | length (µm), effective and group index at a wavelength, loss (dB/cm); the inspector shows its delay |
| **MZI switch** | 2 in, 2 out | θ (internal: π = bar, all of in1 to out1; 0 = cross, all to out2; π/2 = half to each, the default), φ (input phase); the inspector shows the split |
| **Y-splitter** | 1 in, 2 out | — (equal split) |
| **Y-combiner** | 2 in, 1 out | — (only the in-phase part is guided) |
| **Crossing** | 2 in, 2 out | — (no coupling) |
| **Ring resonator** | in → through | coupling, length, effective and group index at a wavelength, loss (dB/cm); the inspector shows the resonance spacing |
| **Add–drop ring** | in, add → through, drop | as the ring resonator; resonant wavelengths go from in to drop (and from add to through) |
| **Splitter tree** | 1 in, 2^levels out | levels |
| **Switch tree** | 1 in, 2^levels out | levels, address (which output gets the light) |
| **Optical DFT** | n in, n out | modes; an MZI mesh computing the discrete Fourier transform |

### Waveguide indices

Delay lines and rings are lengths of waveguide described by two numbers, given at a
reference wavelength (the **at** field, 1550 nm by default):

- **n_eff**, the effective index, sets the phase: a ring of length L resonates where
  n_eff·L = m·λ for a whole number m. The [WDM link](examples.md#4-channel-wdm-optical-link)
  uses L = 20·λ / 2.4 to put each ring on its own channel.
- **n_g**, the group index, sets how quickly that phase changes with wavelength: ring
  resonances sit λ²/(n_g·L) apart (the free spectral range), and light takes n_g·L/c to
  cross. New blocks use silicon's values, n_eff 2.4 and n_g 4.2. Set n_g equal to n_eff
  for a waveguide whose index doesn't change with wavelength.

Away from the reference wavelength the effective index follows
n_eff(λ) = n_eff − (n_g − n_eff)·(λ − λ₀)/λ₀, the usual first-order model in circuit
simulators. Waveguides drawn between boxes are ideal (no length, no loss).

## Component ports

Used inside a layout to give it a boundary when it becomes a [component](components.md).

| Part | Becomes on the box |
|---|---|
| **Input port** | a light input on the left edge (ordered top to bottom) |
| **Output port** | a light output on the right edge |
| **Signal in** | a signal terminal on the top edge (ordered top to bottom, then left to right). In the layout's own tab, click it to switch it between 0 and 1. |
| **Signal out** | a signal terminal on the bottom edge |

Name a port in the inspector; the name shows on the box.
