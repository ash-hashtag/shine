---
title: Changelog
description: What changed in each version of Shine.
order: 16
---

# Changelog

Notable changes in each version. Releases are tagged on
[GitHub](https://github.com/ash-hashtag/shine/releases), where each version's notes come
from this page. What's planned is on the [Roadmap](roadmap.md).

## 0.1.1 (unreleased)

### Added

- Export for other tools (File → Export for other tools…, or `shine export`): a design's
  S-parameters as a Touchstone `.sNp` file for circuit simulators and scikit-rf, and its
  layout as a gdsfactory YAML circuit, which gdsfactory turns into a GDS or OASIS mask with
  a foundry's kit: the way towards a tapeout. Both checked with gdsfactory 9.51 and
  scikit-rf 2.2.
- Components from S-parameter files: Touchstone (`.sNp`, versions 1 and 2, read with
  rust-rf) and Lumerical INTERCONNECT `.dat` files, as foundry kits publish their compact
  models, become black-box components; import them one by one or a folder at a time into
  the asset library, or drop them on the window. Projects are now format 8.
- Scripted components: a component's light and signals computed by JavaScript (QuickJS),
  with its state and the clock, written and applied in the inspector (Component → New
  scripted component). Scripts run in a sandbox with no files, network or clock and with
  time and memory limits (desktop app).
- A reference design: a 16×16 programmable photonic matrix processor (136 MZI cells, 1442
  parts) in the rectangular (Clements) arrangement, programmed as an optical Fourier
  transform; in File → Examples… and as `reference/photonic-matrix-processor.shine`. Tests
  check it against its matrices, and `cargo run --release --example reference_design`
  measures it ([Reference design](reference-design.md)).
- Meshes can be programmed in the rectangular (Clements) arrangement:
  `UnitaryMesh::decompose_rectangular`, N columns deep with balanced losses.
- Large designs draw much faster: a design of 8 600 boxes and 16 600 waveguides went from
  160 ms a frame to 7 ms (off-screen waveguides are skipped, and boxes are found by
  bisection rather than one by one).
- A Laser block's wavelength can be swept.
- SAX netlist export (File → Export for other tools…, `shine export --format sax`): the
  circuit as a Python file for the SAX circuit simulator, with models that compute what
  Shine does; it runs as it is, or with a foundry kit's models swapped in. Checked against
  SAX 0.16 to 10⁻¹⁰.
- Analysis → Optimise… (desktop): Shine adjusts chosen parameters to maximise, minimise
  or reach a power at an output (Nelder–Mead, from argmin), and applies the result.
- A [validation page](validation.md): results against textbook formulas, checked by tests
  with every build, and against PhotonForge.
- Monte Carlo (Analysis → Monte Carlo…, `shine simulate --monte-carlo`): the design under
  fabrication variation of index, coupling, phase and loss, with each output's histogram,
  statistics and yield against a spec; seeded, so results repeat.
- Maps: a sweep can vary a second parameter (Also vary), drawing one output's power over
  both as a heatmap; `shine simulate --sweep2` gives the same as CSV.
- Analysis → Transmission…: a design's S-matrix at any wavelength, and the power, phase
  and group delay of its paths over the wavelengths, plotted and saved as CSV.
- Resonance readouts on wavelength sweeps: free spectral range, width and Q of each
  output, in the sweep window and with `shine simulate … --readouts`.
- Sweeps: plot every output of a design over a range of wavelengths (tune a laser) or of
  one parameter of a part or block, and copy or save the data as CSV. Open from the
  inspector, a part's right-click menu, Analysis → Sweep…, or `?sweep` in a link. Each
  point is a full simulation, components and electronics included.
- Real units: powers in mW (a laser of amplitude 1 emits 1 mW, 0 dBm). Lasers are set by
  their power in mW or dBm; readings show W, mW (0.250 mW below a milliwatt) or nW, or dBm (Edit → Preferences…).
  Attenuators show their loss in dB; detector thresholds are in mW.
- Sweep plots in mW, dBm, or dB relative to the input power, with readouts for every
  curve: peak, minimum, insertion loss and extinction ratio.

- Lasers have a light output: drag it to a box's light input (an MZI, a ring…) to wire
  them with a waveguide, without moving the laser. Projects are now format 4; older
  files open as before.

- Group index for delay lines and rings (with the wavelength the indices are given at), and
  loss for delay lines: ring resonances sit λ²/(n_g·L) apart and delays are n_g·L/c, as on
  real chips. New blocks use silicon's indices (2.4 and 4.2); the inspector shows a ring's
  resonance spacing and a delay line's delay. Projects are now format 5; older rings keep
  their constant index and simulate as before.

- Sweeps of a component box's exported parameters (in the editor and the command line).
- Component parameters: a component exports parameters of the parts and blocks inside it,
  and each box using it sets its own values from the inspector, without opening the
  component. The example Mach–Zehnder component exports its phase. Projects are now
  format 6.

- Frames per second and frame time in the status bar, and a performance graph (View →
  Performance graph) of the time between frames and per frame against 60 and 30 fps, to
  tell a real drop in frames from an impression. Slow frames are also logged.

- The editor works from the keyboard: Tab moves through every control, arrow keys move
  the selection on the canvas (or pan), `]` / `[` select the next or previous part or
  box, + / − zoom, Shift+A adds a part next to the selection without the mouse, and W
  wires the selected item to the next one you select.
- Every control is named for screen readers, and the canvas says what's on it and
  what's selected.
- Installing the desktop app: checking a download, and opening the unsigned builds on
  macOS and Windows (Getting started).
- An asset library (the new **Assets** tab) keeps components for every project, in
  folders, with search by name, description, folder, port or parameter. Shine's example
  components are its *Examples* folder. Add components by dragging them onto a folder,
  from their right-click menu or the Component menu, or by importing files; drag them onto
  the grid to place them.
- Components look the way you choose: a colour, a box shape (rectangle, rounded, capsule,
  cut corners) and a picture instead of the initials. Pictures are shrunk to 128 px and
  stored as WebP; SVGs are kept as they are.
- Ctrl+A (Cmd+A) selects every part and box, as well as A.
- The waveguide attenuator and phase shifter blocks take a signal on top, like their
  free-space counterparts: it multiplies what the attenuator passes (0 blocks it) and
  adds π per unit to the phase. They're now named *Waveguide attenuator* and *Waveguide
  phase shifter*, apart from the free-space parts. The inspector of a driven part or block
  says what the signal does now ("0.5 × 50% → passes 25%").
- The asset library window (View → Asset library…), a window of its own on desktop:
  folders, search, sorting and a grid of tiles that stays quick with thousands of
  components; select several to move or remove them, file tiles by dragging them onto
  folders, and double-click to open one in a tab (as in the Assets tab).
- Import a folder of components (desktop): every `.shinecomp` and `.shine` file in it and
  its subfolders, which become library folders. Dropping a folder on the window does the
  same.
- Component files (`.shinecomp`): export a component with the components it uses, and
  import it into the library or open it in a tab.
- Drag and drop files onto the window: on the left panel they go into the asset library,
  anywhere else they open (a component in a tab of its own, a project in place of the
  current one), and a picture goes on the component being edited. Projects are now format
  7.

### Changed

- Powers below a milliwatt read in mW ("0.501 mW", not "501 µW", which was easy to read
  as 501 mW), and in nW below a microwatt.

- Big designs simulate much faster, and the time grows about in step with the design:
  16 Full adders take 4 ms instead of 145 ms, 32 take 10 ms. Light sources in parts of
  the optics that aren't connected are solved together in one pass, each feedback loop
  only for the sources that reach it, and without the linear-algebra library starting
  threads inside work that already runs in parallel.
- Dragging stays smooth in big designs: when a simulation takes more than about 8 ms, it
  re-runs ten times a second during a drag instead of on every move, and again on
  release.
- Sweeps simulate their points in parallel on desktop: about 5× faster on a 12-core
  machine (the WDM link's 1001-point spectrum takes 80 ms instead of 470 ms). In the
  browser they run in order, as before.
- A new MZI switch starts balanced (θ = π/2), so light in one input leaves both outputs,
  and its inspector shows where the light from in1 goes (at θ = π, all of it to out1).

### Fixed

- A waveguide component with Output pins couldn't be used in another design ("output … is
  already connected"): its pins also shone light into the component itself.

- Dragging a laser's signal pin onto a box's light input warned about connecting a signal
  to light and offered to insert another laser; it now connects the laser's light.
- Windows (Preferences, Sweep, Examples…) could open over the menu bar and block it; they
  now open in the middle and stay below the menus.
- Symbols in the UI (■, ▶, ⚠, arrows…) could show as empty boxes on systems without a
  font that has them; the fonts they need are now bundled.
- Box outputs, beams, ports and waveguides show powers in mW or dBm like everything else.
- Library rows are visible to screen readers.
- Tab never moved between controls in the editor: it opens a component only when a
  component box is selected, and otherwise moves to the next control.
- The inspector's list of what the simulation assumes still said waveguides had no
  dispersion or loss and that powers were relative units.
- Export: every reading as CSV (inspector → Readouts → Copy CSV / Save CSV…), the canvas
  as a PNG image (View → Save canvas image…), and a sweep's plot as a PNG image.
- `shine simulate PROJECT.shine`: readings, wavelength sweeps and parameter sweeps from
  the command line, as text or CSV, with exit codes for scripts and CI.
- Help → About Shine: the version, what's new in it, links to the docs and to report a
  problem, and on desktop a "Check for updates" against the latest GitHub release.
- Crash reports: the desktop app writes a report (message, place, version, system,
  backtrace) and shows it at the next start with a one-click GitHub issue; the browser's
  error page links to one too.
- The session (project, tabs, views, unsaved changes) is kept every few seconds and
  restored at the next start, so closing the window or tab, or a crash, loses at most a
  few seconds of work. File → New project starts over.
- Unsaved changes are shown in the status bar (and the desktop window title), and
  opening another project asks before discarding them. Saves are atomic: a failed save
  leaves the previous file intact.
- The website has `favicon.ico` and WebP icon fallbacks for browsers and apps that don't
  use the SVG icon.
- Powers just under a unit boundary read "1.00 mW", not "1000 µW".
- The browser app's files are loaded under a hash of their contents, so a new version is
  never mixed with a cached old one (and an unchanged one stays cached).
- The example Mach–Zehnder component is labelled MZ, so it isn't mistaken for the MZI
  switch block (which has a θ setting).

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
