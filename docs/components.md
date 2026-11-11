---
title: Components
description: Turn a layout into a reusable box with light ports and signal pins, open it with Tab, and try it on its own.
order: 5
---

# Components

Any layout can become a **component**: a box you can place in other layouts, as many
times as you like. Edits to the component's layout update every place it's used.

![A free-space Mach–Zehnder saved as a component and used twice](img/components.webp)

## Making one

- **+ New component** (Assets → *This project*, the parts browser, or the **+** tab
  button) creates a component with one light input and output and one signal input and
  output, and asks for its name, initials and description.
- Or turn any open layout into one: **Component → Save as component…**

The box shows the **initials**; hovering it shows the **description** and its ports.

## How its boxes look

**Edit component…** also sets how its boxes look, with a preview:

- **Colour**: tick *Own colour* and pick one; otherwise a colour comes from the name.
- **Shape**: *Rectangle*, *Rounded*, *Capsule* or *Cut corners*.
- **Picture**: shown on the box instead of the initials. Choose one (PNG, JPEG, WebP, GIF,
  BMP or SVG) or drop it on the window while the dialog is open. Pictures are shrunk to
  fit 128 pixels and stored as WebP, so they add a few kilobytes to the project; an SVG is
  stored as it is (up to 512 KB) and stays sharp at every zoom. **Remove** goes back to
  the initials.

The colour, shape and picture are part of the component, so they travel with it into
other projects and the asset library.

## Ports and pins

What the box exposes comes from port parts inside the layout:

| Inside the layout | On the box | Order |
|---|---|---|
| Input port | light input, left edge | top to bottom |
| Output port | light output, right edge | top to bottom |
| Signal in | signal terminal, top edge | top to bottom, then left to right |
| Signal out | signal terminal, bottom edge | top to bottom, then left to right |

Name each port in the inspector (for example `a`, `b`, `cin`); the names show on the box
and in tooltips. Without names they are `in1…`, `out1…`, `s1…`, `q1…`.

An Input port injects light travelling in its arrow's direction; an Output port reads
the light that reaches it. Signal in outputs the value arriving at the box; Signal out
takes a value and passes it out of the box.

## Opening a component

Select a box and press **Tab**, or double-click it, to open its layout in a tab. The
inspector shows where it's used. You can't place a component inside itself, directly or
through other components.

## Parameters

A component can **export** parameters of the parts and blocks inside it, so each box using
it can set its own value without opening the component. Open the component, select a part
(a phase shifter, a laser, a ring…), and under **Export to boxes using this component**
click the parameter to export; give it a name there if you like. Click it again to stop
exporting.

Select a box using the component, and its inspector lists the exported parameters with
their values. A parameter you haven't set shows *default* and uses the value inside the
component; drag or type to set it for this box only, and **↺** to go back to the default.
The example **Mach–Zehnder interferometer** (MZ) exports its phase: set it to π on a box
to switch its light to the other output. Exported parameters can also be
[swept](sweeps.md) on a box, from its right-click menu or the sweep window, and from the
[command line](command-line.md).

## Trying a component on its own

In a component's own tab nothing drives its Signal in pins, so they read 0. **Click a
Signal in pin** to switch it between 0 and 1; a lamp next to it shows the value and the
simulation follows. This lets you test, for example, the Full adder's truth table
without building anything around it. These test values aren't saved and don't affect
the component where it's used.

![The Full adder component opened in its own tab](img/full-adder.webp)

## Managing components

In the **Project** list (Parts tab, below the parts), right-click a document to open,
rename, edit its component details, or delete it. **Component → Stop sharing as
component** turns it back into a plain layout. A component that is still used somewhere
can't be deleted or unshared until its boxes are removed.

## The asset library

The **Assets** tab keeps components for every project, not just this one: on desktop in
a file next to Shine's settings, in the browser in the page's storage. It has folders:

- **This project**: the open project's components.
- **Examples**: the components that ship with Shine (Full adder, Mach–Zehnder
  interferometer, Fabry–Pérot filter, 1→4 splitter).
- Your own folders, *My components* to start with.

Drag a component onto the grid (or click it, then the grid) to place it, and
**double-click** it to open it in a tab. A library component is copied into the project
the first time, with any components it uses, and placing it again reuses that copy.
Right-click a library component to **Open in a tab**, **Export…** it, **Move to folder**
(an existing one or a new one), or **Remove from library**; projects that use it keep
their copy. The tab lists up to 40 components per folder; **All … in the library** shows
the rest in the library window.

### The library window

**View → Asset library…** (or **⊞ Library** in the Assets tab) opens every component in
a grid, made for large libraries. On desktop it is a window of its own, like a file
picker, that you can move and resize beside the editor; in the browser it opens inside
the page. It opens at its full size and keeps whatever size you give it.

- **Folders** on the left: *All*, *This project*, *Examples* and yours, with how many each
  holds.
- **Search** by name, description, folder, port or parameter, and **sort** by name or
  newest first. Only the rows on screen are drawn, so thousands of components stay
  quick.
- **Click** a tile to see its details on the right (ports, parameters, folder) with
  **Place**, **Open in a tab** and **Export…**; **double-click** opens it in a tab.
  **Ctrl+click** and **Shift+click** select several, to move or remove them together.
- **Place** (in the details) and then a click on the grid places it. In the browser you
  can also drag a tile onto the grid. Drag a tile onto a folder to file it there; **Move
  to folder** in the details does the same, and a name typed under it makes a new folder.
- **Import folder…** (desktop) adds every `.shinecomp` and `.shine` file in a folder and
  its subfolders. Each folder becomes a library folder (*Optics*, *Optics / Rings*…), and
  components already in the library aren't added twice. Dropping a folder on the window
  does the same.

Double-clicking a component's box on the grid opens it in a tab too.

To keep a component in the library:

- drag it from *This project* onto a library folder, or right-click it → **Add to
  library**, or **Component → Add to asset library**;
- **Import…** (or **Component → Import to asset library…**) `.shinecomp` files, or a
  `.shine` project to add every component in it;
- or drop those files on the left panel.

Drag a library component onto another folder to move it there.

### Sharing components

**Component → Export component…** (or **Export…** in a right-click menu) saves a
`.shinecomp` file: the component with every component it uses. Send it to someone and they
can **drop it on Shine's window to open it in a tab of its own** (it's added to their open
project), or drop it on the left panel to keep it in their asset library. A component
that is already in the project, unchanged, isn't copied twice; a different one with the
same name gets a new name (*Full adder 2*).

## Components from data: S-parameters

A component can be a black box known only by its S-parameters, measured or simulated
elsewhere: a foundry kit's compact models, an FDTD simulation, a measurement. Shine reads
**Touchstone** files (`.s2p`, `.s4p`…, version 1 or 2) and **Lumerical INTERCONNECT**
files (`.dat`, as kits such as SiEPIC EBeam publish them). Bring them in like any
component: **Import files…** or **Import folder…** in the asset library, or drop them on
the window (on the left panel to keep them, elsewhere to open one in a tab). A folder of
them, a kit's models for example, becomes a library folder.

Each file becomes a component with a pin for every port. Light in Shine runs one way, so
its ports are split into inputs (left) and outputs (right): by the sides a Lumerical file
gives, otherwise the split that carries the most light from inputs to outputs (a Y-branch's
port 1 in, ports 2 and 3 out; a coupler's two sides). Between the file's wavelengths the
values are interpolated (magnitude and phase); outside them, the nearest one is used.
Reflections and light running backwards aren't modelled. Its inspector shows the
wavelength range and points. Touchstone files are read in the desktop app; Lumerical
files in the browser too.

## Components from code: JavaScript

**Component → New scripted component** (or **+ New scripted component** in the Assets
tab) makes a component whose light and signals a short JavaScript program computes: a
device you can describe with a formula, a small IC with a state and a clock, or both. Its
script is in the inspector; edit it and press **Apply**: the box takes the ports the
script declares. The template:

```js
const component = {
  name: "Variable splitter",
  light: { inputs: ["in"], outputs: ["out1", "out2"] },
  signals: { inputs: ["split"], outputs: ["ticks"] },
  state: 0,

  // The field from each light input to each output at a wavelength in nm:
  // one row per output, each entry a number or [re, im].
  transmission(wavelength, signals) {
    const k = Math.min(Math.max(signals.split ?? 0, 0), 1);
    return [[Math.sqrt(1 - k)], [Math.sqrt(k)]];
  },
  // The signal outputs, by name (or as a list in order).
  outputs(signals, state) { return { ticks: state }; },
  // On each clock tick: the next state.
  tick(signals, state) { return state + 1; },
};
```

- `signals` holds the signal inputs by name; a component's unconnected input reads 0, as
  everywhere in Shine.
- `transmission` can depend on the signals, so a script can be a modulator or a switch.
- `state` is anything JSON can hold; `tick` runs on each clock tick, like a register.
- A mistake shows with its line, in the inspector or as the simulation's error.

Scripts run in **a sandbox**: [QuickJS](https://bellard.org/quickjs/) with the language
only. There are no modules, files, network, processes or timers, no clock (`Date`), and
`Math.random` gives the same numbers every run, so a design always simulates the same.
Each call can run for 250 ms and the engine use 32 MB; a script that runs longer or uses
more is stopped with an error. Scripts run in the desktop app: in the browser, a design
that uses one opens but shows an error instead of simulating.
