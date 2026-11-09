---
title: Changelog
description: What changed in each version of Shine.
order: 15
---

# Changelog

Notable changes in each version. Releases are tagged on
[GitHub](https://github.com/ash-hashtag/shine/releases), where each version's notes come
from this page. What's planned is on the [Roadmap](roadmap.md).

## 0.1.1 (unreleased)

### Added

- Sweeps: plot every output of a design over a range of wavelengths (tune a laser) or of
  one parameter of a part or block, and copy or save the data as CSV. Open from the
  inspector, a part's right-click menu, Analysis → Sweep…, or `?sweep` in a link. Each
  point is a full simulation, components and electronics included.
- Real units: powers in mW (a laser of amplitude 1 emits 1 mW, 0 dBm). Lasers are set by
  their power in mW or dBm; readings show mW, µW, nW or W, or dBm (Edit → Preferences…).
  Attenuators show their loss in dB; detector thresholds are in mW.
- Sweep plots in mW, dBm, or dB relative to the input power, with readouts for every
  curve: peak, minimum, insertion loss and extinction ratio.

- Lasers have a light output: drag it to a box's light input (an MZI, a ring…) to wire
  them with a waveguide, without moving the laser. Projects are now format 4; older
  files open as before.

### Changed

- A new MZI switch starts balanced (θ = π/2), so light in one input leaves both outputs.

### Fixed

- Dragging a laser's signal pin onto a box's light input warned about connecting a signal
  to light and offered to insert another laser; it now connects the laser's light.
- Windows (Preferences, Sweep, Examples…) could open over the menu bar and block it; they
  now open in the middle and stay below the menus.
- Symbols in the UI (■, ▶, ⚠, arrows…) could show as empty boxes on systems without a
  font that has them; the fonts they need are now bundled.
- Box outputs, beams, ports and waveguides show powers in mW or dBm like everything else.
- Library rows are visible to screen readers.

## 0.1.0 (2026-11-09)

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
