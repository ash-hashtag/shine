---
title: Editing
description: Placing parts, selecting and moving, the add menu, the parts browser, and every keyboard and mouse shortcut.
order: 2
---

# Editing

The controls follow Blender's conventions. Press **F1** (or **?**) in the app for the
full list.

## The grid

Everything sits on a square grid. Free-space parts (lasers, mirrors, detectors…) take
one cell and are drawn as themselves. Built-in blocks and components are **boxes** that
take several cells: light ports on their left and right edges, signal terminals on the
top and bottom edges. The **cell size** (Inspector → Simulation, 10 µm by default) sets
the physical distance light travels between cells, which matters for interference.

## Adding parts

There are four ways, all placing into the cell you point at:

- **Drag** a row from the library onto the grid.
- **Click** a library row, then click the grid (repeat to place more; Esc stops).
- **Shift+A** (or right-click empty space) opens the **add menu** at the cursor.
- The **parts browser** (▦ next to the library search).

### The library

![The library folders](img/library.webp)

The left panel has two tabs. **Parts** groups the parts into folders: *Light &
detection*, *Free-space optics*, *Electronics & I/O*, *Waveguide blocks* and *Component
ports*. Click a folder to open or close it. Typing in the search box shows matching parts
from every folder in one list. Below the parts is the **Project** list of every document.

**Assets** holds [components](components.md#the-asset-library): *This project*'s own,
then the **asset library**, kept for every project: Shine's *Examples* and your own
folders. Its search finds components by name, description, folder, port or parameter.
The add menu and the parts browser list them too, as *This project* and *Asset library*.

### The add menu

![The Shift+A menu with the Electronics & I/O category open](img/add-menu.webp)

Hover a category to open its parts beside it, then click one. Or just start typing:
the menu turns into a search, and **Enter** adds the first match. Scrolling over the
menu scrolls the menu, not the grid. **Esc** or a click on the grid closes it.

### The parts browser

![The Parts & components browser](img/browser.webp)

The browser shows every part and component as a tile with its description, folder by
folder. Click a
tile and then click the grid, or drag the tile onto the grid. **+ New component** starts
a [component](components.md).

## Selecting, moving, deleting

- **Click** selects; **Shift+click** adds or removes; **drag on empty space** draws a
  selection box. Clicking a waveguide or signal wire selects it.
- **Drag** a part or box to move it, or press **G** and move the mouse (**X** / **Y** lock
  the axis, click or Enter confirms, right-click or Esc cancels). Parts can't be dropped
  onto occupied cells.
- **R** / **Shift+R** rotate lasers by 90° and mirrors and splitters by 45°.
  Double-clicking a part rotates it too.
- **Shift+D** duplicates the selection (with the wires between the copies) and starts
  moving it.
- **X** or **Delete** removes the selection, or the selected wire.
- **Ctrl+Z** / **Ctrl+Shift+Z** undo and redo.

## Navigating

- **Wheel** or **pinch** zooms at the cursor; **trackpad scrolling** and **middle-drag**
  pan; **Ctrl+wheel** / **Shift+wheel** pan sideways / up and down; **Ctrl+middle-drag**
  zooms.
- **Home** frames everything; **.** (period) frames the selection.
- The canvas zooms from 1% to 800%. Far out, it draws less so big designs stay quick:
  under 25% boxes are plain outlines, and under 10% areas of boxes become tiles, darker
  where there are more and tinted where light passes; zoom in to see each part again.

## Exporting

- **Readings:** under *Readouts* in the inspector, **Copy CSV** and **Save CSV…** give
  every reading of the design (detectors, output ports, displays, registers, and the light
  leaving each box) as CSV: name, value, unit. `shine simulate --csv` prints the same
  ([Command line](command-line.md)).
- **Images:** **View → Save canvas image…** saves the canvas, as you see it, as a PNG
  image; a sweep's plot has **Save image…** in its window.
- **Sweeps:** CSV from the [sweep window](sweeps.md#exporting).

On desktop a save dialog asks where (with `SHINE_EXPORT_DIR` set, files go straight into
that folder); in the browser files download.

## Preferences

**Edit → Preferences…** sets how Shine looks and behaves, and is remembered between
sessions (a settings file on desktop, the browser's storage on the web):

| Setting | Options |
|---|---|
| Theme | System, light or dark |
| Interface size | The size of menus, panels and text, 50 to 250%; the keyboard and wheel zoom only the canvas |
| Canvas zoom | Open designs at the zoom you last used on the canvas (wheel, pinch, + and −), or fit each design to the window |
| Grid, port names | Show or hide the grid and the names on component boxes |
| Performance | Frames per second, frame time (drawing included) and simulation time in the status bar; the performance graph (also View → Performance graph) |
| Light colours | Real colours where visible (infrared and ultraviolet in distinct false colours), a distinct colour per wavelength, or one colour; use lasers' own colours |
| Channels | Draw each wavelength on a waveguide or beam as its own strand |
| Beams, readings | Direction arrows; detector powers on the canvas |
| Power in | mW (W above a watt, nW below a microwatt) or dBm, for readings and laser power |
| New lasers | Their wavelength, typed or from presets |
| Simulation | Live simulation for new documents; clock speed |

## Keyboard and screen readers

Everything in the editor works from the keyboard. Tab and Shift+Tab move through the
menus, panels and windows, and Space or Enter presses the focused button. On the canvas,
`]` and `[` step the selection through the parts and boxes, the arrow keys move what's
selected (or pan the view when nothing is), G with the arrows and Enter makes a longer
move, Shift+A opens the add menu next to the selection (type to search, Enter to place),
and + and − zoom. To wire, select the start and press W, step to the end with `]` / `[`
and press Enter: it connects the first free output of the start to the first free input
of the end that match (light before signals), and Esc gives up. Keys go to the focused control first: while a slider has the focus, the
arrows change its value.

Every control has a name for screen readers (the label beside it, or a description for
buttons that show only a symbol), and the canvas describes what's on it and what's
selected. A test checks that no control on any panel or window is left unnamed.

## Keyboard and mouse reference

| Keys | Action |
|---|---|
| Middle drag / Shift+Middle drag | Pan |
| Ctrl+Middle drag | Zoom |
| Wheel / pinch | Zoom at cursor |
| Trackpad scroll | Pan |
| Home | Frame all |
| . (period) | Frame selected |
| Left click | Select |
| Shift+Left click | Extend / toggle selection |
| Left drag on empty space | Box select (Shift extends) |
| Left drag on component | Move it |
| Double click | Rotate a part / open a component |
| Click a number's bit | Toggle that bit |
| A or Ctrl+A / Alt+A / Ctrl+I | Select all / none / invert |
| G | Grab (move); X / Y constrain axis |
| Enter / Left click | Confirm grab |
| Esc / Right click | Cancel grab |
| R / Shift+R | Rotate clockwise / counter-clockwise |
| Shift+D | Duplicate and move |
| Arrow keys / Shift+arrows | Move the selection (or a grab) 1 / 5 cells; with nothing selected, pan |
| + / − or Ctrl+= / Ctrl+− | Zoom the canvas in / out |
| Ctrl+0 | Fit the design to the window |
| ] / [ | Select the next / previous part or box |
| W, then ] / [ and Enter | Wire the selected item to another, first free ports |
| X / Delete | Delete selected |
| Shift+A / Right click | Add menu / context menu |
| Tab / Double click | Open the selected component's definition in its own tab (otherwise Tab moves to the next control) |
| Drag from a port or terminal | Wire it: light ports take waveguides, square terminals take signals |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |
| Space | Toggle simulation |
| T / N | Toggle toolbar / properties |
| Ctrl+N / Ctrl+O / Ctrl+S | New / open / save |
| F5 | Reload the project from its file (desktop) |
| F1 / ? | This help |
