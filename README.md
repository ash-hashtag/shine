# Shine

Optics simulator for designing photonic chips

## Running

```sh
cargo run --release                                  # editor
cargo run --release -- --log-file logs/shine.log     # also log to a file
SHINE_LOG=debug cargo run                            # log filter (or --log-level, RUST_LOG)
cargo run --release --no-default-features --example matmul   # library only
```

Logs always go to stderr; `--log-file <path>` / `SHINE_LOG_FILE` adds a file (appended, directories created).

## Editor controls (Blender-style)

| Input | Action |
|---|---|
| Middle drag, Shift+Middle drag / trackpad scroll | Pan |
| Wheel, pinch, Ctrl+Middle drag | Zoom at cursor |
| Ctrl+Wheel / Shift+Wheel | Pan horizontally / vertically |
| Home / `.` | Frame all / selected |
| Click, Shift+click, drag on empty space | Select, extend, box select |
| Drag a component, or G (then X/Y to lock an axis) | Move |
| R / Shift+R, double click | Rotate |
| Shift+D, X/Delete, Shift+A, right click | Duplicate, delete, add menu, context menu |
| A / Alt+A / Ctrl+I | Select all / none / invert |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |
| Tab, double click | Open a component's definition |
| Drag from a port | Draw a waveguide (drag a connected input to move its waveguide) |
| Space, T, N, F1 | Simulate, library, inspector, shortcuts |

## Documents and components

A project holds **free-space layouts** (beams on a grid) and **schematics** (blocks wired
with waveguides), each in its own tab. Any document can be saved as a component
(Component → Save as component…): give it a name, initials and a description. Its ports
are the *Input/Output port* parts of a layout, or the *Input/Output pins* of a schematic,
ordered top to bottom.

In a schematic, components appear as boxes with their initials; hover for the description,
drag from a port to another port to draw a waveguide, and press **Tab** (or double-click)
to open the component's definition. Edits there update every instance. Waveguides are ideal
by default; switch to *From drawing* in the inspector, or give a waveguide a fixed length,
to include propagation phase. Projects save as one JSON file (File → Save).

## Library

`shine::photonics` is usable without the GUI: build a `Circuit` from components or
reusable blocks (`photonics::blocks`: splitter/combiner trees, all-pass and add-drop
rings, interleavers, MZI switch trees, optical DFT, WDM demux, SVD matrix multiplier),
then `compile` it per wavelength and run batches of inputs.
