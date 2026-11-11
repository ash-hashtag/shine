---
title: Roadmap
description: Where Shine is going, as goals and milestones rather than fixed versions or dates.
order: 15
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

## Milestone: production ready

What a tool people rely on every day needs, before more features.

- [x] **Never lose work**: unsaved changes are marked and confirmed before they're
      discarded; saves are atomic; the session is saved continuously and restored after a
      crash or a closed tab
- [x] **Crash reports**: a crash is logged with what's needed to fix it, and the next start
      says what happened
- [x] **About and updates**: the version, what's new, and where to report problems
- [x] **Command-line simulation**: `shine simulate project.shine --sweep … --csv`, for
      scripts and CI
- [x] **Export**: sweep data and readings as CSV, plots and the canvas as images
- [x] **Sweep a component's exported parameters**
- [x] **Speed on big designs**: simulation time that grows gently with the number of
      components (measured with `examples/scale_bench` and `examples/frame_bench`)
- [x] **Accessibility**: every control named for screen readers; keyboard use throughout
- [ ] **Installable builds**: signed and notarised desktop apps (until then, [Getting
      started](getting-started.md#installing-the-desktop-app) explains opening the
      unsigned builds)

## Milestone: answer the questions engineers ask

- [x] **Wavelength sweeps**: transmission versus wavelength for any design
- [x] **Parameter sweeps**: sweep a part's parameter and plot the outputs
- [x] **Real units**: power in mW and dBm, losses in dB, insertion loss and extinction
      ratio readouts

## Milestone: circuits with real numbers

Still circuit-level and still instant, but with the numbers real devices have, when you
want them.

- [x] **Waveguide model**: group index (so ring spacing and delays match real devices) and
      propagation loss for delay lines and rings; connections stay ideal
- [ ] **Validation page**: simulated results plotted against textbook formulas (rings,
      MZI, Fabry–Pérot, couplers, the DFT mesh)
- [ ] **Two polarisations** per port

## Milestone: work with other tools

Shine makes a design work quickly; the rest of the industry's tools take it from there,
and feed it their components.

- [x] **S-parameter export** (Touchstone `.sNp`) of any design with light ports, for
      circuit simulators, scikit-rf and measurement software
- [x] **Layout for gdsfactory**: the design as gdsfactory components, placements and
      routes, which gdsfactory turns into a GDS or OASIS mask with a foundry's kit: the
      way to a tapeout ([Other tools and tapeout](interop.md))
- [x] **`shine export`** on the command line, for scripts and CI
- [x] **S-parameter import**: Touchstone `.sNp` (versions 1 and 2) and Lumerical `.dat`
      files from a foundry kit (PDK), an FDTD tool or a measurement, used as black boxes;
      a folder of them becomes an asset library folder
- [ ] **Netlist export** for circuit simulators that take one (gdsfactory/SAX netlists,
      Lumerical INTERCONNECT)
- [ ] **Choose a foundry kit** when exporting a layout: its component names and sizes
      instead of the generic ones

## Milestone: analytics and graphs

- [x] **Resonance readouts** on wavelength sweeps: FSR, width and Q, checked against the
      textbook ring
- [x] **Readouts from the command line** (`shine simulate … --readouts`)
- [ ] **Phase and group delay** plots, from the same sweep
- [ ] **Two-parameter sweeps**: a map of an output over two parameters
- [ ] **S-matrix view**: how every input reaches every output, at a wavelength
- [ ] **Monte Carlo**: how a design holds up to fabrication variation; histograms and yield

## Milestone: custom components

- [x] **Scripted components**: a component's light, signals and state written in
      JavaScript (QuickJS), run in a sandbox with no files, network or clock and with time
      and memory limits; so libraries can hold small ICs and parametric devices
- [x] **Standard component libraries**: folders of Touchstone and Lumerical files (as the
      open foundry kits publish their compact models) imported into the asset library
- [ ] **Parameters for scripts**: values each box sets, as for components
- [ ] **Scripts in the browser** (QuickJS built for WebAssembly)

## Later: realistic simulation (separate)

- Physics-level models of components from their geometry and materials, kept separate
  from the fast circuit simulator
- Time-domain simulation: modulation, bandwidth, eye diagrams and noise

## Later: from circuit to chip

- The first step is done through gdsfactory (above). Later: placement that respects each
  kit's real sizes and rules, and drawing GDS or OASIS directly for the simplest cases

## Ideas

- A photon-counting and few-photon (quantum) teaching mode
