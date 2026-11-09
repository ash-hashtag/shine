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
  Detector 26,5  976 µW (-0.11 dBm)
  Detector 31,5  2.20 µW (-26.58 dBm)
  Detector 36,5  977 µW (-0.10 dBm)
  Detector 41,5  974 µW (-0.12 dBm)
  Display 26,9   11
```

It prints every reading of the first document that isn't a component: detectors and
output ports (in mW and dBm), displays and registers. `--doc NAME` picks another
document, and `--csv` prints the readings as CSV (name, value, unit).

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
| `--box-outputs` | include the light leaving boxes |
| `--clock N` | tick the clock N times first, so registers hold values |
| `--csv` | readings as CSV |
| `--out FILE` | write to a file instead of the terminal |
| `--help` | the options |

`shine --version` prints the version.

## In scripts and CI

The exit status is 0 on success, 1 when the design doesn't simulate (for example an
oscillating loop) and 2 for a mistake in the command (a missing file, an unknown option or
name); messages go to standard error. A check in CI can simulate a design and compare
the CSV with an expected one.

On Windows the app has no console window, so write results with `--out FILE` (or
redirect the output to a file).
