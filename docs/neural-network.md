---
title: Optical neural network
description: A small AI inference chip in Shine, a two-layer optical neural network that reads handwritten digits, simulated end to end and checked against SAX and PhotonForge.
order: 9.8
---

# An optical neural network that reads handwritten digits

Most of the work in running a neural network is multiplying vectors by matrices, and a mesh
of Mach–Zehnder interferometers does that with light in the time light takes to cross the
chip. This example is a complete small inference chip built that way: a two-layer optical
neural network that reads handwritten digits. Shine simulates all of it, light, detectors
and electronics together. Open it with **File → Examples… → Digit recognizer (optical
neural network)**.

It reads **93.6%** of 450 handwritten digits it was never trained on correctly, the same
as the trained network. An electronic network of the same size reads 92.0%.

## Using it

Light and data flow left to right:

1. **The digit** is the 4×4 grid of value inputs at the top left: a handwritten digit
   shrunk to 16 pixels, each from 0 (blank) to 1 (ink), as a filled bar. Each shows the
   light it lets into the first layer. Click along a bar to change a pixel.
2. **The laser** (*LS*) is split in two (*Y*), and each half 16 ways (*1×N*): one set of 16
   lights for each layer.
3. **Layer 1**: 16 linear modulators put the pixels on the lights; the layer (*W*)
   multiplies them by a trained 16×16 matrix.
4. **Neurons**: 16 photodetectors with electronics (*ƒ n1* … *ƒ n16*) read the layer's
   outputs and turn each power into a drive, clamp(gain × power, 0, 1): the network's
   nonlinearity.
5. **Layer 2**: the neurons drive 16 more modulators on the laser's other half; the layer
   multiplies by a trained 10×16 matrix, one output per digit.
6. **Readout**: electronics (*ƒ argmax*) find the brightest of the 10 outputs and the
   display shows it: the digit the chip read.

Press Tab on a layer to see inside it, and Tab again on its meshes and MZI cells.

## How it is built

| Part | What it is | Size |
|---|---|---|
| **Linear modulator** | A push-pull Mach–Zehnder modulator with its driver: the driver sets the two arms' heaters to ±(π/2 − asin v), so the field out is exactly v times the field in, with no change of phase | 9 blocks |
| **Layer** | A weight matrix W as W = U Σ V†: a mesh of MZIs for V†, an attenuator per output for Σ (the largest passing everything), a mesh for U. Both meshes are in the rectangular (Clements) arrangement, with balanced paths | Layer 1: 2 × 120 MZIs; layer 2: 120 + 45 MZIs |
| **Neuron** | A function cell reading light: a photodetector, gain and clamp, driving the next modulator | |
| **Chip** | 16 pixels, 32 modulators, 2 layers, 16 neurons, readout and display | 4 698 parts, 465 MZIs |

The network was trained (`reference/digits/train.py`, with JAX) on exactly what the chip
computes:

```text
h     = clamp(|W1 x|² / H, 0, 1)    layer 1, photodetectors, neurons
o     = |W2 h|²                      layer 2, photodetectors
digit = argmax(o)
```

So the weights suit the hardware: detection, which squares the field, is part of the
network rather than an error to correct. Shine builds the chip from the trained weights
(`src/panels/optics/digits.rs`): it decomposes each matrix into meshes, sets every heater
and attenuator, and works out the neurons' gain from the laser power and the losses along
the way.

## Checked three ways

Shine simulates the whole chip for each of the 450 test digits. Its 10 output powers are
compared with the trained network computed digitally, and the chip's two optical layers,
exported as SAX netlists, are run in two other simulators with the same electronics
around them (`reference/digits/compare.py`):

| | Digits read right | Same digit as Shine | Largest difference in the scores |
|---|---|---|---|
| Trained network (digital) | 421 / 450 (93.6%) | 450 / 450 | 9 × 10⁻¹¹ |
| Shine, the whole chip simulated | 421 / 450 (93.6%) | | |
| SAX 0.16, the layers' netlists | 421 / 450 (93.6%) | 450 / 450 | 9 × 10⁻¹¹ |
| PhotonForge 2.1, the layers rebuilt from its own models | 421 / 450 (93.6%) | 450 / 450 | 9 × 10⁻¹¹ |

The differences are the rounding of the saved scores. How long each took, on a laptop
(AMD Ryzen 5 7430U), November 2026:

| | Time |
|---|---|
| Shine: both layers' matrices, from the design | 12 ms |
| Shine: the whole chip, light, detectors and electronics, for one digit | 25 ms |
| SAX: both layers' matrices | 3.5 s to build the circuits, 4.1 s to solve (2.8 s again) |
| PhotonForge: both layers' matrices | 8.7 s to solve (10 ms again, from its cache) |

SAX and PhotonForge simulate the optics only: the neurons and the readout ran in NumPy
around their matrices. Shine simulates the electronics too, as part of the circuit.

To run the comparison yourself:

```sh
cargo run --release --example digits -- --export out --scores out/shine.json
pip install sax photonforge
python reference/digits/compare.py out reference/digits/test-digits.json --photonforge
```

## How realistic it is

- **The architecture is real.** Programmable MZI meshes with optoelectronic neurons
  (detect, compute the nonlinearity electronically, modulate again) are how coherent
  optical neural networks are built and demonstrated in research.
- **The components are ideal**: lossless 50:50 couplers, perfect heaters, a fixed
  waveguide loss, no noise, no fabrication error. On a real chip, meshes need calibrating,
  detectors add noise and the electronics have limited precision.
- **It is small.** 16 inputs means each digit is a 4×4 thumbnail; real accelerators use
  larger meshes and many layers, and keep the weights in the mesh while streaming inputs
  at gigahertz rates through the modulators.
- **The data**: the network is trained and tested on the UCI handwritten digits (Alpaydin
  & Kaynak, 1998, CC BY 4.0), the 8×8 digits that ship with scikit-learn. They are close
  in spirit to MNIST; MNIST's licence (CC BY-SA) would put share-alike conditions on the
  test digits kept in the repository.

To train it again: `pip install numpy jax scikit-learn`, then
`python reference/digits/train.py reference/digits` and
`cargo run --example digits -- --write`.
