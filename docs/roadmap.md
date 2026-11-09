---
title: Roadmap
description: Where Shine is going, what's being built in the next 0.1 releases, and what comes after.
order: 14
---

# Roadmap

Shine today is a sound simulator for learning, teaching and early design: exact
interference and resonance, reusable components, and the electronics around them. The
roadmap is about making it useful for engineers designing real photonic systems, in this
order: answer the questions engineers ask, get the numbers right, get the data out, then
connect to real foundry models. Releases stay small: each 0.1.x patch adds a step or two,
and the minor version moves to 0.2 only once the 0.1 line is complete.

What's already shipped is in the [Changelog](changelog.md). Ideas and requests are welcome
as [GitHub issues](https://github.com/ash-hashtag/shine/issues).

## 0.1.0: released 2026-11-09

- [x] One editor for free-space optics, component boxes, waveguides and signals
- [x] Exact frequency-domain solver: interference, resonance, feedback loops
- [x] Reusable, nested components; registers, a clock and function cells
- [x] Coherent and independent light sources; real and false colours
- [x] Desktop (Linux, Windows, macOS) and browser builds; MCP server; documentation

## 0.1.x: useful to engineers (next)

### 0.1.1

- [x] **Wavelength sweeps in the editor**: transmission versus wavelength, in dB, for any
      design, from a chosen source or input to chosen outputs
- [x] **Parameter sweeps**: sweep a part's parameter and plot the outputs
- [x] **Real units**: laser power in mW or dBm, readings in mW and dBm, losses in dB,
      insertion loss and extinction ratio readouts

### 0.1.2

- [ ] **Export**: sweep data as CSV (done), readouts as CSV, plots and the canvas as images
- [ ] **Release basics**: an About box with the version, release notes, and instructions
      for unsigned binaries

### 0.1.3

- [ ] **Waveguide physics**: group index and dispersion, propagation loss in dB/cm,
      wavelength-dependent coupling, so ring spacing and delays match real devices

### 0.1.4

- [ ] **Validation page**: simulated results plotted against textbook formulas (rings,
      MZI, Fabry–Pérot, couplers, the DFT mesh)

## 0.2: connect to real designs

- **S-parameter import** (Touchstone `.sNp`): use foundry kit (PDK) and FDTD results as
  black-box components
- **Command-line simulation**: `shine simulate project.shine --sweep … --csv`, for scripts
  and CI
- **Monte Carlo**: how a design holds up to fabrication variation
- **Two polarisations** per port

## Later

- Layout export (GDS), for example through gdsfactory
- Time-domain simulation: modulation, bandwidth, eye diagrams and noise
- Signed and notarised desktop builds
- A photon-counting and few-photon (quantum) teaching mode
