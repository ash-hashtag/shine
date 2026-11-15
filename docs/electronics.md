---
title: Electronics and the clock
description: Number and value inputs, displays, registers, the clock, threshold detectors and function cells, with the full expression reference.
order: 6
---

# Electronics and the clock

Shine's electronics are deliberately small: enough to drive optics and read it back,
not a general logic simulator. Light does the optical work; detectors, registers and
function cells provide decisions and memory.

## Inputs and displays

A **Number input** sends a number as bits, most significant first (leftmost terminal).
Click its lamps to toggle bits, or set the value in the inspector. A **Value input**
sends one value, usually between 0 and 1, for analog control: driving a heater (it adds
value × π to a phase shifter) or a modulator, or setting an attenuator (it passes that
share of the light). It shows its value as a filled bar, with its name (set in the inspector); when it
drives a box with one light output, such as a modulator, it also shows the light that
box lets out. Click along it to set it (left end 0, right end 1, in steps of 0.05), or
set it exactly in the inspector. A **Display** shows
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
| Functions | `abs sqrt floor ceil round exp ln sin cos asin acos atan`, `pow(x, y)`, `bit(x, k)` (bit k of x), `clamp(x, lo, hi)`, `min(…)`, `max(…)`, `sum(…)`, `argmax(…)` (0-based index of the largest) |

Examples:

- XOR of two bits: `in1 ^ in2`
- Majority of three: `in1 + in2 + in3 >= 2`
- A light level from a 2-bit number: `sqrt(in1 * 2 + in2) / 2`
- Which of four detectors is brightest: `argmax(p1, p2, p3, p4)`

## External programs

> **⚠ A risky part, for when Shine can't do something itself.** An *External program* block
> runs a program on your computer, with your permissions: it can read, change or delete your
> files and use the network. A project can name any program, so one from someone else could
> run anything. Shine never runs one without asking.

The block asks a program for its outputs from its inputs: logic you have in Python or
TypeScript, or a tool Shine doesn't have. In its inspector set the **program** (run
directly, not through a shell: `python3`, `node`, or a path), its **arguments** (one per
line, such as the script's path) and the names of its signal **inputs** and **outputs**.
It can take and give light too, as a function cell does: each **light in** gives the
program the power arriving there (mW), and each **light out** emits the field amplitude the
program answers for it, at the block's **wavelength**.

**Permission.** The first time a simulation reaches the block, Shine shows what it would run
and asks: *Don't run it* or *Run it this session*. Until you allow it, the block gives
nothing and the simulation says why. Permission lasts until Shine closes and is never saved,
so the next session asks again. `shine simulate` runs programs only with
`--allow-processes`. The MCP server (for AI assistants) and the browser never run them.

**The protocol** is lines of text on the program's standard input and output, numbers
separated by spaces:

1. Shine starts the program once and writes `shine 1 <inputs> <outputs>`, the counts of its
   inputs and outputs; the program answers `ok`.
2. For each evaluation, Shine writes a line of input values (the signal inputs, then the
   light inputs' powers), and the program answers a line with as many output values as it
   was told (the signal outputs, then the light outputs' amplitudes), in order.
3. When Shine is done (or closes), it closes the program's input; the program should exit.

The program keeps running between evaluations, so it's fast enough to call often. Shine may
ask for the same inputs more than once, or not again, so give the same outputs for the same
inputs. An answer that takes over 5 seconds, or isn't numbers, stops the program with an
error (it starts again next time). What the program writes to its standard error goes to
Shine's log. Remember to flush after each answer.

In Python (`python3`, argument `xor.py`):

```python
import sys

sys.stdin.readline()          # "shine 1 2 1"
print("ok", flush=True)
for line in sys.stdin:
    a, b = (round(float(v)) for v in line.split())
    print(a ^ b, flush=True)
```

In TypeScript or JavaScript (`node`, argument `xor.mjs`; with `node --experimental-strip-types`
or `npx tsx` for a `.ts` file):

```js
import { createInterface } from "node:readline";

let started = false;
for await (const line of createInterface({ input: process.stdin })) {
  if (!started) { started = true; console.log("ok"); continue; }
  const [a, b] = line.split(" ").map((v) => Math.round(Number(v)));
  console.log(a ^ b);
}
```

A component holding a program is always simulated in full, never kept as a function of its
inputs (see [Simulation](simulation.md#components)), so its program is asked each time.

## How it settles

Each time something changes, Shine evaluates the electronics from the detector readings,
re-solves the light with the new drive values, and repeats until nothing changes
(usually 2–5 rounds; the inspector shows how many). A loop that keeps flipping, such as a
NOT gate feeding itself, is reported as *did not settle*. See
[How the simulation works](simulation.md).
