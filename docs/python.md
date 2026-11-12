---
title: Python
description: Design, simulate and analyse photonic circuits from Python with the shine-photonics package, and open what you build in the Shine app.
order: 13.1
---

# Python

The `shine-photonics` package is Shine as a Python library: build components and designs,
simulate them, sweep, analyse, optimise and export, from a script or a notebook, with no
window. It runs the same engine as the app. What you build is a normal Shine project: save
it and open it in the app to look at it, or open a file made in the app and script it.

```sh
pip install shine-photonics
```

## A first circuit

```python
import shine_photonics as shine

project = shine.Project()
chip = project.new_design("Ring filter")

laser = chip.laser(wavelength_nm=1550, power_mw=1)
ring = chip.ring(coupling=0.05, length_um=100)
detector = chip.detector()
laser.to(ring).to(detector)          # light flows laser → ring → detector

print(chip.simulate().power_mw(detector))      # 0.9987…

spectrum = chip.sweep(laser, "wavelength", 1540, 1560, points=2001)
r = spectrum.readouts("Detector")
print(r.fsr_nm, r.q)                           # the ring's resonances: 5.72 nm apart

project.save("ring.shine")                     # open it in the app
```

`part.to(next)` connects a part's first free light output to the next part's first free
light input. For anything else, connect ports by name:

```python
coupler = chip.coupler(coupling=0.5)
chip.connect(coupler.port("out2"), detector.port("in"))
```

## Components

A component is a design other designs place as one part. Its light and signal pins are its
ports, and the parameters it exposes are what each copy sets:

```python
mzm = project.new_component("Modulator")
light_in, light_out = mzm.light_input("in"), mzm.light_output("out")
split, join = mzm.y_splitter(), mzm.y_combiner()
heater = mzm.phase_shifter(phase=3.14159)
upper, lower = mzm.waveguide(length_um=100), mzm.waveguide(length_um=100)
light_in.to(split).to(heater).to(upper)
mzm.connect(split.port("out2"), lower.port("in"))
mzm.connect(upper.port("out"), join.port("in1"))
mzm.connect(lower.port("out"), join.port("in2"))
join.to(light_out)
mzm.expose("bias", heater, "phase")

link = project.new_design("Link")
modulator = link.place(mzm).set("bias", 0.0)
laser, detector = link.laser(), link.detector()
laser.to(modulator).to(detector)
```

A pin faces into its component: an input pin's only port is `out`, an output pin's `in`.

Components can also be known by data or by code:

```python
project.new_model_from_touchstone("Coupler", open("coupler.s4p", "rb").read())
project.new_model_from_lumerical("Y-branch", open("y_branch.sparam").read())

class Filter:
    def matrix(self, wavelength_nm):            # rows: outputs, columns: inputs
        a = min(1.0, abs(wavelength_nm - 1560) / 10)
        return shine.Matrix(rows=1, cols=1, values=[shine.Complex(re=a, im=0.0)])

project.new_model_from_function("Filter", ["in"], ["out"], [1550.0, 1555.0, 1560.0], Filter())
```

A function-written component is sampled at the wavelengths you give and kept as a table,
so it saves with the project and opens in the app like any other.

### Matrices as light

Any matrix becomes a component that multiplies by it, built from MZI meshes, as optical
neural networks and signal processors are:

```python
rows = [[0.5, -1.0, 2.0], [0.25, 0.0, 1.0]]
w = shine.Matrix(rows=2, cols=3, values=[shine.Complex(re=x, im=0.0) for r in rows for x in r])
layer = project.new_weight_layer("Weights", w)   # inputs in1…in3, outputs out1, out2
shine.layer_scale(w)                              # σmax: the layer computes W x / σmax
project.new_unitary_mesh("Mixer", u)              # a unitary as one mesh
```

A weight layer is W = U Σ V†: two meshes of MZIs (the rectangular, Clements, arrangement)
with an attenuator per mode between them. Light can't be amplified, so it computes W x
divided by W's largest singular value, σmax, which its `description()` gives too. Any
shape, real or complex. The meshes are components of their own, to open in the app.

## Finding your way around

Everything can say what it has, and errors say what would have worked:

```python
>>> ring.ports()
[PortInfo(name='in', kind=<PortKind.LIGHT: 0>, direction=<Direction.IN: 0>),
 PortInfo(name='through', kind=<PortKind.LIGHT: 0>, direction=<Direction.OUT: 1>)]
>>> [(p.name, p.unit, p.value) for p in ring.params()]
[('coupling', '', 0.05), ('length', 'µm', 100.0), ('n_eff', '', 2.4), ('n_g', '', 4.2), ('loss', 'dB/cm', 10.0)]
>>> ring.port("drop")
shine_photonics.ShineErrorException: “Ring resonator” has no port “drop”; it has: in, through
>>> print(chip.describe())
```

Parts are named as the app names them: "Ring resonator", the second one "Ring resonator 2";
`design.find_part(name)` finds one. Parameters have the inspector's and sweeps' names
(`coupling`, `length`, `n_eff`, `θ`…), and `part.set(name, value)` sets any of them.

## Analysis

| Method | What it gives |
|---|---|
| `design.simulate()` | `power_mw(part)`, `output_power_mw(port)`, `signal(port)`, `display(part)`, `readings()` |
| `design.transmission(wavelengths_nm)` | between a component's pins: `field`, `power`, `phase`, `group_delay_ps`, `matrix` |
| `design.sweep(part, param, start, stop, points)` | `series(output)`, `readouts(output)` (resonances, FSR, width, Q), `to_csv()` |
| `design.monte_carlo(runs, seed, variation)` | `values(output)`, `stats(output)`, `yield_of(output, min_mw, max_mw)` |
| `design.optimise(variables, output, goal, target_mw, steps)` | the best values; `design.apply(variables, values)` sets them |
| `design.to_touchstone`, `to_sax`, `to_gdsfactory` | files for [other tools](interop.md) |

Readings are named by part ("Detector") or by part and port ("Ring resonator.through").
Conventions are the app's: fields are complex amplitudes with |E|² in mW; wavelengths in
nm, lengths in µm, phases in radians, loss in dB or dB/cm; light runs one way.

## Looking at it in the app

- `project.save("file.shine")` writes a normal Shine file: open it in the desktop app or in
  the browser app.
- `design.show()` saves the project to a temporary file and opens the desktop app on that
  design (the app is `shine`, or the program in the `SHINE_APP` environment variable).
- Parts added in code are laid out when the project is saved, left to right as light
  flows; `part.place_at(column, row)` fixes a position.
- `shine_photonics.Project.open("file.shine")` reads a project made in the app.

The same API is available in [TypeScript](typescript.md) and in [Rust](library.md).
