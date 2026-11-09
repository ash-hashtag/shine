---
title: Roadmap
description: Where Shine is going, as goals and milestones rather than fixed versions or dates.
order: 14
---

# Roadmap

Shine today is a sound simulator for learning, teaching and early design: exact
interference and resonance, reusable components, and the electronics around them. The
goal is to make it useful for engineers designing real photonic systems.

This is a list of goals and milestones, not a timeline. Items land in whichever release
is next once they're ready, and anyone is welcome to pick one up. What each release
actually contains is in the [Changelog](changelog.md). Ideas and requests are welcome as
[GitHub issues](https://github.com/ash-hashtag/shine/issues).

## Foundations (done)

- [x] One editor for free-space optics, component boxes, waveguides and signals
- [x] Exact frequency-domain solver: interference, resonance, feedback loops
- [x] Reusable, nested components; registers, a clock and function cells
- [x] Coherent and independent light sources; real and false colours
- [x] Desktop (Linux, Windows, macOS) and browser builds; MCP server; documentation

## Milestone: answer the questions engineers ask

- [x] **Wavelength sweeps**: transmission versus wavelength for any design
- [x] **Parameter sweeps**: sweep a part's parameter and plot the outputs
- [x] **Real units**: power in mW and dBm, losses in dB, insertion loss and extinction
      ratio readouts
- [ ] **Export**: sweep data as CSV (done), readouts as CSV, plots and the canvas as images

## Milestone: get the numbers right

- [ ] **Waveguide physics**: group index and dispersion, propagation loss in dB/cm,
      wavelength-dependent coupling, so ring spacing and delays match real devices
- [ ] **Validation page**: simulated results plotted against textbook formulas (rings,
      MZI, Fabry–Pérot, couplers, the DFT mesh)
- [ ] **Two polarisations** per port

## Milestone: connect to real designs

- [ ] **S-parameter import** (Touchstone `.sNp`): use foundry kit (PDK) and FDTD results
      as black-box components
- [ ] **Command-line simulation**: `shine simulate project.shine --sweep … --csv`, for
      scripts and CI
- [ ] **Monte Carlo**: how a design holds up to fabrication variation

## Milestone: easy to adopt

- [ ] An About box with the version and release notes
- [ ] Instructions for unsigned binaries, then signed and notarised desktop builds

## Ideas for later

- Layout export (GDS), for example through gdsfactory
- Time-domain simulation: modulation, bandwidth, eye diagrams and noise
- A photon-counting and few-photon (quantum) teaching mode
