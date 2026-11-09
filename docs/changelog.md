---
title: Changelog
description: What changed in each version of Shine.
order: 14
---

# Changelog

Notable changes in each version. Releases are tagged on
[GitHub](https://github.com/ash-hashtag/shine/releases), where each version's notes come
from this page. What's planned is on the [Roadmap](roadmap.md).

## 0.1.0 (unreleased)

The first release.

### Editor

- One grid for free-space optics, component boxes, waveguides and electrical signals, with
  Blender-style controls (G, R, Shift+D, Shift+A, box select, undo and redo).
- A library in collapsible folders with search, a Parts & components browser, and an add
  menu with a submenu per category.
- Reusable components: any layout becomes a box with light ports and signal pins; Tab opens
  its definition; edits update every use. In its own tab, a component's input pins can be
  clicked to try it alone.
- Wiring by dragging between ports. Connecting light to a signal (or the reverse) warns and
  offers to insert the converter: a detector or a laser.
- Preferences, saved between sessions: theme, grid, port names, light colours, channel
  strands, beam arrows, readings, default laser wavelength, live simulation, clock speed.
- Light colours: real colours for visible wavelengths, distinct false colours for infrared
  and ultraviolet, or a colour per laser; one strand per wavelength on shared waveguides.

### Simulation

- An exact frequency-domain solver: each circuit compiles per wavelength into SIMD
  multiply-adds, with an LU factorisation for each feedback loop.
- Laser sources: independent lasers add power; lasers on a shared source (A–H) interfere.
  All sources at a wavelength are solved together in one vectorised pass.
- Electronics: threshold detectors, driven lasers, phase shifters and attenuators, number
  inputs, displays, clocked registers, and function cells with an expression language.
  Optics and electronics settle together; oscillating loops are reported.
- Free-space parts, waveguide blocks (couplers, MZIs, rings, add–drop rings, splitter and
  switch trees, the optical DFT) and nested components in one circuit.

### Examples

- A 4-channel WDM optical link, an 8-bit adder and an accumulator (as an electro-optic
  teaching model), free-space interferometers and cavities, and chip circuits with
  spectra: ring filters, interleaver, WDM demultiplexer, switch tree, Fourier transform,
  matrix multiplier.

### Files, platforms and integrations

- Projects in a compact binary `.shine` format (format 3); older files are upgraded on open.
- Desktop apps for Linux (x86-64, ARM64), Windows and macOS (Apple silicon, Intel), and the
  same editor in the browser.
- `shine-mcp`, a Model Context Protocol server for AI assistants.
- The `shine::photonics` Rust library.
- Documentation with screenshots, search, and plain-text versions for language models.
