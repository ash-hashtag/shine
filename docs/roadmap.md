---
title: Roadmap
description: Where Shine is going, as goals and milestones rather than fixed versions or dates.
order: 14
---

# Roadmap

Shine is **circuit first**: you connect components that behave as specified and see what
the whole circuit does, instantly. That makes prototyping fast. Get the idea working with
ideal parts, then add real numbers where they matter, and fix real problems once the
design works. Realistic, physics-level simulation and turning a circuit into a chip that
can be fabricated come later, as separate steps that build on a working circuit rather
than slowing down the first one.

This is a list of goals and milestones, not a timeline. They're built one at a time, each
tested and checked against expectations before the next. Items land in whichever release
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

## Milestone: circuits with real numbers

Still circuit-level and still instant, but with the numbers real devices have, when you
want them.

- [x] **Waveguide model**: group index (so ring spacing and delays match real devices) and
      propagation loss for delay lines and rings; connections stay ideal
- [ ] **S-parameter import** (Touchstone `.sNp`): measured or simulated components from a
      foundry kit (PDK) or an FDTD tool, used as black boxes
- [ ] **Monte Carlo**: how a design holds up to fabrication variation
- [ ] **Validation page**: simulated results plotted against textbook formulas (rings,
      MZI, Fabry–Pérot, couplers, the DFT mesh)
- [ ] **Command-line simulation**: `shine simulate project.shine --sweep … --csv`, for
      scripts and CI
- [ ] **Two polarisations** per port

## Milestone: easy to adopt

- [ ] An About box with the version and release notes
- [ ] Instructions for unsigned binaries, then signed and notarised desktop builds

## Later: realistic simulation (separate)

- Physics-level models of components from their geometry and materials, kept separate
  from the fast circuit simulator
- Time-domain simulation: modulation, bandwidth, eye diagrams and noise

## Later: from circuit to chip

- Help converting a circuit into a layout that can be fabricated: real components from a
  foundry kit, routed waveguides, and export to OASIS or GDS

## Ideas

- A photon-counting and few-photon (quantum) teaching mode
