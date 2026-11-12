---
title: Command line
description: Simulate saved projects from a terminal, in scripts and in CI, and get readings or sweeps as CSV.
order: 10
---

# Command line

The desktop `shine` program also simulates saved projects without opening a window:

```sh
shine simulate wdm.shine
```

```text
4-channel WDM optical link · wdm.shine · settled in 3 rounds
  Detector 26,5  0.976 mW (-0.11 dBm)
  Detector 31,5  0.00220 mW (-26.58 dBm)
  Detector 36,5  0.977 mW (-0.10 dBm)
  Detector 41,5  0.974 mW (-0.12 dBm)
  Display 26,9   11
```

It prints every reading of the first document that isn't a component: detectors and
output ports (in mW and dBm), displays and registers. `--doc NAME` picks another
document, and `--csv` prints the readings as CSV (name, value, unit).

## Inputs

`--input FILE` sets the design's inputs (its value inputs and number inputs) from a
stream of numbers, so a script can feed a design its data; `-` reads them from standard
input. Numbers are separated by spaces, commas or lines, and `#` starts a comment.
`--inputs` lists the inputs in the order the numbers set them:

```sh
shine simulate adder.shine --inputs
echo "7 8" | shine simulate adder.shine --input -
```

Several samples' numbers one after another simulate each in turn, printing a CSV row per
sample: its number, then every reading. A number input takes its number rounded into the
range of its bits. `--only TEXT` keeps just the readings whose names contain TEXT, again
for more. [Classifying your own pictures](neural-network.md#at-full-size) with the
Fashion-MNIST chip works this way, through `examples/mnist.py`.

## Sweeps

A sweep prints CSV, the same as **Save CSV…** in the editor's [sweep window](sweeps.md):
the swept value, then one power column (mW) per output.

```sh
# Tune the first laser from 1520 to 1570 nm in 51 steps.
shine simulate wdm.shine --wavelength 1520:1570:51 --out spectrum.csv

# Sweep a parameter of a part or block.
shine simulate wdm.shine --sweep '24,4.coupling=0.01:0.3'
```

`--wavelength FROM:TO[:POINTS]` tunes the first laser, or the one `--laser ITEM` names.
`--sweep ITEM.PARAM=FROM:TO[:POINTS]` sweeps any parameter listed on the
[Sweeps](sweeps.md#parameter-sweeps) page. Without POINTS, wavelength sweeps take 201
points and parameter sweeps 101. `--box-outputs` adds the light leaving every block.

ITEM can be:

- a part's label, such as a port's name (`in`);
- a part or block name when only one has it (`Phase shifter`);
- a grid cell, `x,y`, as shown in the inspector and the status bar (`24,4`). When a name
  is shared, the error lists the cells to choose from.

## Options

| Option | Effect |
|---|---|
| `--doc NAME` | the document to simulate |
| `--wavelength FROM:TO[:POINTS]` | sweep a laser's wavelength (nm) |
| `--laser ITEM` | the laser to tune |
| `--sweep ITEM.PARAM=FROM:TO[:POINTS]` | sweep a parameter |
| `--sweep2 ITEM.PARAM=FROM:TO[:POINTS]` | with `--wavelength` or `--sweep`: vary a second parameter too (default 21 points), for a map as CSV, a row per point |
| `--monte-carlo N` | simulate N copies under fabrication variation: each sample's powers as CSV, or with `--readouts` each output's nominal, mean, σ, 5 %, 95 %, min and max |
| `--seed S` | the Monte Carlo seed (default 1): the same seed, the same samples |
| `--vary-neff X`, `--vary-coupling X`, `--vary-phase X`, `--vary-loss X` | the variation's σ (defaults 0.002, 0.03, 0.05 rad, 0.1) |
| `--box-outputs` | include the light leaving boxes |
| `--readouts` | with a sweep: each output's peak, minimum, insertion loss, extinction, and resonances (FSR, width, Q) as CSV, instead of the curves |
| `--input FILE` | set the inputs from numbers in FILE (`-`: standard input); several samples, a CSV row each |
| `--inputs` | list the inputs in the order `--input` sets them |
| `--only TEXT` | only the readings whose names contain TEXT (again for more) |
| `--clock N` | tick the clock N times first, so registers hold values |
| `--csv` | readings as CSV |
| `--out FILE` | write to a file instead of the terminal |
| `--help` | the options |

`shine --version` prints the version.

## Exporting

`shine export` writes a design in a format other tools read (see
[Other tools and tapeout](interop.md)):

```sh
shine export ring.shine                                   # S-parameters: ring.s4p
shine export ring.shine --wavelength 1540:1560:2001 --out ring.s4p
shine export chip.shine --format gdsfactory               # layout: chip.pic.yml
shine export ring.shine --format sax                      # SAX netlist: ring.py
```

| Option | Effect |
|---|---|
| `--format touchstone`, `sax` or `gdsfactory` | the format (default: from `--out`'s extension, else touchstone) |
| `--doc NAME` | the design (default: the first with light ports) |
| `--wavelength FROM:TO[:POINTS]` | Touchstone and SAX: the wavelengths in nm (default 1500:1600:401); a SAX file prints the transmission at them |
| `--spacing UM` | gdsfactory: room between components, µm (default 40) |
| `--out FILE` | where to write (default: next to the project, named after the design) |

It prints what it wrote, with a note for anything that didn't carry over (free-space parts
in a layout, for example). A Touchstone file's name must say its port count: `.s4p` for
four ports.

## In scripts and CI

The exit status is 0 on success, 1 when the design doesn't simulate (for example an
oscillating loop) and 2 for a mistake in the command (a missing file, an unknown option or
name); messages go to standard error. A check in CI can simulate a design and compare
the CSV with an expected one.

On Windows the app has no console window, so write results with `--out FILE` (or
redirect the output to a file).
