---
title: Electronics and the clock
description: Number inputs, displays, registers, the clock, threshold detectors and function cells, with the full expression reference.
order: 6
---

# Electronics and the clock

Shine's electronics are deliberately small: enough to drive optics and read it back,
not a general logic simulator. Light does the optical work; detectors, registers and
function cells provide decisions and memory.

## Number inputs and displays

A **Number input** sends a number as bits, most significant first (leftmost terminal).
Click its lamps to toggle bits, or set the value in the inspector. A **Display** shows
the bits on its inputs as lamps and as a decimal number; the inspector lists every
display's value under *Displays*.

## Registers and the clock

A **Register** has *d* inputs, a *write* input and *q* outputs. Its outputs hold the
stored value. On each **clock tick** it stores its *d* inputs, unless *write* is
connected and 0.

The clock lives in the inspector under *Simulation*:

- **Step** advances one tick.
- **Run** ticks twice a second until you press **Pause**.
- **Reset** clears every register and the tick count.

Registers make feedback possible. The [Photonic accumulator](examples.md#photonic-accumulator)
adds a step to a register on every tick.

## Threshold detectors

A detector outputs the power it receives. Tick **bit** in its inspector to give it a
comparator: it outputs 1 when the power reaches the threshold (0.5 by default), else 0.
This is where an analog optical result becomes a digital decision.

## Function cells

A **Function cell** computes its outputs from expressions you write. Use it for logic you
don't want to build from parts, to model a receiver circuit, or to drive light from a
calculation.

In the inspector:

- **Signal inputs**: how many `in1`, `in2`, … terminals it has.
- **Light inputs**: how many light ports it has on the left; `p1`, `p2`, … are the
  powers received there, in mW.
- **Signal outputs**: one expression per `out` terminal.
- **Light outputs**: one expression per light output; the value is the field
  **amplitude** (power = amplitude² mW) at the cell's wavelength.

A red `!` next to an expression means it doesn't parse; hover it for the reason.

### Expression reference

| | |
|---|---|
| Variables | `in1`, `in2`, … signals; `p1`, `p2`, … received light power; `t` clock ticks |
| Numbers | `3`, `0.5`, `1e-3` |
| Arithmetic | `+ - * / %`, `**` (power), unary `-` |
| Bits | `<< >> & \| ^ ~` on whole numbers |
| Comparison and logic | `< <= > >= == !=`, `&&`, `\|\|`, `!`; true is 1, false is 0 |
| Choice | `cond ? a : b` |
| Functions | `abs sqrt floor ceil round exp ln sin cos`, `pow(x, y)`, `bit(x, k)` (bit k of x), `clamp(x, lo, hi)`, `min(…)`, `max(…)`, `sum(…)`, `argmax(…)` (0-based index of the largest) |

Examples:

- XOR of two bits: `in1 ^ in2`
- Majority of three: `in1 + in2 + in3 >= 2`
- A light level from a 2-bit number: `sqrt(in1 * 2 + in2) / 2`
- Which of four detectors is brightest: `argmax(p1, p2, p3, p4)`

## How it settles

Each time something changes, Shine evaluates the electronics from the detector readings,
re-solves the light with the new drive values, and repeats until nothing changes
(usually 2–5 rounds; the inspector shows how many). A loop that keeps flipping, such as a
NOT gate feeding itself, is reported as *did not settle*. See
[How the simulation works](simulation.md).
