---
title: Wiring light and signals
description: Beams, waveguides and signal wires; why light and signals never connect directly, and the converters that join them.
order: 4
---

# Wiring light and signals

A circuit carries two different things, and Shine keeps them apart:

| | Light | Signals |
|---|---|---|
| What it is | an optical field: amplitude, phase, wavelength | an electrical value: usually 0 or 1, sometimes a number |
| Drawn as | beams (free space) and thick coloured waveguides | thin right-angled wires; orange = 1, grey = 0, blue = other values |
| Ports | round, on the left (in) and right (out) of boxes | square, on the top (in) and bottom (out) |
| Fan-out | one waveguide per port | one output can drive many inputs |

## Beams

Free-space parts talk through beams. A laser shoots a beam along its direction; mirrors,
splitters and other parts redirect it; detectors and the backs of mirrors absorb it.
Beams travel along rows, columns and 45° diagonals.

A beam also connects to boxes: a beam that reaches the left edge of a box on the row of a
light input enters that input, and light leaving an unconnected output of a box goes out
of its right edge as a beam. Light hitting a box anywhere else is absorbed.

## Waveguides

Drag from a box's light output (round, right edge) to another box's light input (round,
left edge). The waveguide is drawn as a curve, coloured by the light it carries. Its
length follows the drawing, so moving boxes changes the phase. Each port takes one
waveguide; a new one replaces the old. Click a waveguide and press X to delete it.

## Signal wires

Drag from a square output terminal (bottom edge) to a square input (top edge), or the
other way round. An input has one driver; an output can drive many inputs. Wires are
routed with right angles, and wires between the same rows get separate lanes so buses
stay readable.

Parts with signal terminals:

- **Laser** (input on top): the amplitude is multiplied by the signal.
- **Phase Shifter** (input on top): adds π per unit.
- **Attenuator** (input on top): multiplies the transmission.
- **Detector** (output below): the received power, or 0/1 with *bit* on.
- the electronics blocks, function cells, and components' **Signal in/out** pins.

## Converting between light and signals

Light and signals never connect directly. If you drag a wire from one to the other, the
wire turns amber while you hover, and dropping it opens a small offer that explains what is
needed, with a button that inserts and wires it:

- **Light → signal: a detector.** Shine places a detector in the beam leaving that light
  output and wires its output to the signal input. For displays and registers it
  switches the detector to 0/1 output.
- **Signal → light: a laser.** Shine places a laser in front of that light input and
  wires the signal to its amplitude, so 1 turns it on and 0 off.

If the cell it needs is taken, it says which cell to clear. In real chips these are a
photodiode with a receiver circuit, and a modulator on a laser.
