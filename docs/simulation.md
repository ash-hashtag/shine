---
title: How the simulation works
description: What Shine solves exactly, what it models simply, and the limits to keep in mind when reading results.
order: 9
---

# How the simulation works

## Light: exact, linear, per wavelength

Every optical part is a **scattering matrix**: it maps the complex field (amplitude and
phase) on its inputs to the field on its outputs. Shine connects these into one circuit
and solves the steady state **exactly** as a linear system, once per laser wavelength.

- **Interference** comes out right because fields add, not powers.
- **Loops and resonances** (rings, cavities, a Fabry–Pérot) are solved directly, not by
  bouncing light a fixed number of times.
- **Phase from distance**: a beam crossing one cell picks up the phase of one cell size
  (Inspector → Simulation → Cell size). Waveguides use their drawn length with an
  effective index of 2.4.
- **Coherence.** Only lasers on the same *shared source* (A–H) interfere, like light split
  from one laser. Independent lasers (the default) and different wavelengths are solved
  separately and their powers added, as with real separate lasers. All independent
  sources at one wavelength are solved together in one SIMD pass, so this stays fast.
  Built-in laser blocks and function-cell light share one source.

Fields are normalised so |E|² is power: a laser of amplitude 1 delivers power 1.

## Electronics: settled against the light

Electronics are evaluated from the detector readings, the light is re-solved with the new
drive values (laser amplitudes, phase shifts, transmissions, function-cell light), and
this repeats until nothing changes. Registers only change on a clock tick, so feedback
through a register is well defined. A combinational loop that keeps flipping is reported
as an error instead of guessed.

## Components

A component box is simulated from its layout every time, nested as deep as you like;
there's no cached approximation. A beam enters a box only on the row of a light input,
travelling right; unconnected light outputs leave the box as beams to the right.

## Colours

Colours are only a display choice (Edit → Preferences): visible wavelengths can show their
real colour, infrared and ultraviolet get distinct false colours, or every wavelength gets
its own colour, or all light one colour; any laser can have its own colour. A waveguide
carrying several wavelengths is drawn as one strand per channel, and the inspector lists
which colour is which wavelength.

## What is simplified

Read results with these in mind:

- **Ideal parts.** Splitters, mirrors and couplers are lossless unless you add loss;
  detectors are noiseless and instantaneous; lasers have no linewidth or noise.
- **Steady state only.** No time dynamics: no pulse shapes, rise times, bandwidth or
  propagation delay. "Clock ticks" are logical steps, not a time scale.
- **Grid optics.** Beams are rays on a grid in 8 directions; there is no diffraction,
  beam width or polarisation. The lens passes light straight through.
- **Waveguides** have a fixed effective index (2.4) and no dispersion or bend loss.
- **Electronics** are idealised logic and arithmetic, not circuits: no voltages,
  thresholds' noise margins, or timing.

So Shine is right about *interference, routing, resonance and wavelength behaviour* of a
design, and is a good place to learn and prototype ideas. It is not a substitute for a
device-level (FDTD, mode solver) or circuit-level (SPICE, time-domain) simulator when
designing a chip for fabrication.

## Performance

Circuits compile per wavelength into a flat program: parts on straight paths become SIMD
multiply-adds over batches of inputs, and each feedback loop gets an LU factorisation
computed once. Sweeps over wavelength run in parallel on desktop builds.
