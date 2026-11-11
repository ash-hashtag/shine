---
title: Roadmap
description: Where Shine is going, as goals and milestones rather than fixed versions or dates.
order: 15
---

# Roadmap

Shine is **circuit first**: you build logic and circuits from components that behave as
specified and see what the whole design does, instantly. It isn't a sketchpad to leave for
"serious" tools: a design made in Shine is meant to become a real, top-of-the-line chip.

The order matters. First the design works: its logic, its circuits, its numbers. Only
when it heads for a real chip do real-world effects come in: reflections and cavities,
noise, time-domain behaviour, polarisation, the foundry's rules. For each of those, Shine
either does it well enough to rely on and keep up to date, or hands the design over
cleanly, through standard formats, to the tools that specialise in it, so the way from a
Shine design to a tapeout is always there. Keeping Shine simple is part of being
competitive: every feature has to be one we can maintain.

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
- [x] **Validation page**: results against textbook formulas (couplers, MZIs, rings,
      Fabry–Pérot, waveguides, the DFT mesh), checked with every build, and against
      PhotonForge ([Validation](validation.md))
- [x] **SAX comparison**: Shine's netlists solved in SAX agree to 10⁻¹⁰
      ([Validation](validation.md))
- [ ] **More independent comparisons**: Lumerical INTERCONNECT
- [x] **Optimisation**: Shine finds the parameter values (couplings, lengths, phases) that
      maximise, minimise or reach a target power at an output (Analysis → Optimise…,
      desktop)
- [ ] **Optimise a response**: targets over a band of wavelengths (a filter's shape, a
      resonance's position), and several outputs at once

## Milestone: design at scale

- [x] **A reference design at a real size**: a 16×16 programmable photonic processor
      (120 MZIs, 1 410 parts), checked against its matrices with every build and
      measured up to 128×128 (83 000 parts) ([Reference design](reference-design.md))
- [x] **Large designs stay quick to look at**: drawing and navigating a design of 8 600
      boxes and 16 600 waveguides takes a few milliseconds a frame
- [x] **An AI inference chip**: a two-layer optical neural network reading handwritten
      digits, simulated whole and checked against SAX and PhotonForge
      ([Optical neural network](neural-network.md))
- [ ] **Program a mesh from a matrix in the editor**: choose or type a matrix, and Shine
      sets the mesh's heaters (the decomposition is there; the panel isn't)
- [ ] **Smaller SAX netlists**: leave out components' pins (zero-phase pass-throughs, a
      third of the instances in a large design)
- [x] **Simulate in the background**: editing a design whose simulation takes longer than
      a frame stays smooth, with results arriving when they're ready (desktop)

- [ ] **Python API**: Shine's engine and project files from Python, so designs fit into
      the scripts and flows engineers already use (gdsfactory, SAX, notebooks)

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
- [x] **SAX netlist export**: the circuit as a SAX netlist with models that compute what
      Shine does, to run as it is or with a foundry kit's models
- [ ] **Netlist export for Lumerical INTERCONNECT**
- [ ] **Choose a foundry kit** when exporting: Shine's own blocks written as the kit's
      equivalent cells (layout) and compact models (SAX, S-parameters), e.g. SiEPIC EBeam
      through gdsfactory's UBC kit; designs stay in Shine's components and the kit is
      chosen only at the end

## Milestone: analytics and graphs

- [x] **Resonance readouts** on wavelength sweeps: FSR, width and Q, checked against the
      textbook ring
- [x] **Readouts from the command line** (`shine simulate … --readouts`)
- [x] **Phase and group delay** of every path between a design's ports, checked against
      a waveguide's n_g·L/c and a ring's resonant delay
- [x] **Two-parameter sweeps**: a map of an output over two parameters, in the sweep
      window and with `shine simulate --sweep2`
- [x] **S-matrix view**: how every input reaches every output, at a wavelength
      (Analysis → Transmission…)
- [x] **Monte Carlo**: how a design holds up to fabrication variation; histograms,
      statistics and yield against a spec, in the editor and with `--monte-carlo`
- [ ] **Monte Carlo inside components**, and of a sweep's readouts (how far a resonance
      moves)

## Milestone: custom components

- [x] **Scripted components**: a component's light, signals and state written in
      JavaScript (QuickJS), run in a sandbox with no files, network or clock and with time
      and memory limits; so libraries can hold small ICs and parametric devices
- [x] **Standard component libraries**: folders of Touchstone and Lumerical files (as the
      open foundry kits publish their compact models) imported into the asset library
- [ ] **Parameters for scripts**: values each box sets, as for components
- [ ] **Scripts in the browser** (QuickJS built for WebAssembly)

## Milestone: towards the chip

When a working design heads for tapeout, the real world comes in. Each item is done in
Shine where we can keep it reliable, and otherwise handed to specialised tools through the
exports above.

- [x] **A route to tapeout**: the layout through gdsfactory and a foundry's kit
- [ ] **Light both ways**: reflections, back-scatter and cavities on the chip, so foundry
      models are used whole (today their reflections are dropped on import) and lasers and
      Fabry–Pérot effects can be modelled
- [ ] **Time domain**: modulation, bandwidth, bit patterns, eye diagrams and noise, for
      transceivers and modulators
- [ ] **Two polarisations** and more than one mode per port
- [ ] **Temperature**: index and loss changing with temperature (heaters, drift)
- [ ] **Foundry rules**: placement with each kit's real sizes, design-rule checks, and a
      check that the layout matches the circuit
- [ ] **Physics-level models** of components from their geometry, through tools that
      specialise in it (FDTD, mode solvers) rather than built in

## Ideas

- A photon-counting and few-photon (quantum) teaching mode
