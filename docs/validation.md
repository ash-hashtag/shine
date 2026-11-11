---
title: Validation
description: Shine's results against textbook formulas and an independent simulator, checked with every build.
order: 9.5
---

# Validation

Shine's circuit simulation is exact for the models it uses: every result below is
compared with a closed-form, textbook formula, and the comparisons run as tests
(`tests/validation.rs`) with every build, so this page can't drift from the code.

## Against the textbook

| Circuit | Formula | Agreement |
|---|---|---|
| Directional coupler | bar 1 − κ, cross κ, the cross port π/2 ahead | 10⁻¹² |
| MZI switch | in1 → out1: sin²(θ/2), nothing lost | 10⁻¹² |
| Unbalanced MZI (arms 100 and 300 µm) | fringes λ²/(n_g ΔL) apart | 1 % (sampling) |
| All-pass ring (κ 0.05, 100 µm, 3 dB/cm, n_g 4.2) | FSR λ²/(n_g L); dip (r − a)²/(1 − ra)²; width (1 − ra)λ²/(π n_g L √(ra)); Q = λ/width | FSR 1 %, dip 2 %, width and Q 5 % (sampling) |
| Add–drop ring, unequal couplings, with loss | through (r₂a − r₁)²/(1 − r₁r₂a)²; drop κ₁κ₂a/(1 − r₁r₂a)² at resonance | 10⁻⁶ |
| Waveguide (2 mm, n_g 4.2, 3 dB/cm) | group delay n_g L/c = 28.0 ps; loss 0.6 dB | 10⁻⁶ |
| Fabry–Pérot bench (mirrors R = 0.9) | peak 1; valley ((1 − R)/(1 + R))²; spacing λ²/(2L); finesse π√R/(1 − R) = 29.8 | 3 % (sampling) |
| Optical DFT, N = 2 to 16 | unitary; every input reaches every output with 1/N of its power | 10⁻⁹ |
| 32 cascaded unequal MZIs (lossless) | output power sums to the input at every wavelength | 10⁻⁹ |

Where the agreement is a percentage, the formula is exact and the tolerance is the
sweep's sampling: a resonance's width is measured between samples.

## Against an independent simulator

The same circuits were built in Flexcompute's PhotonForge 2.1 (its analytic circuit
models, run locally) and compared with Shine's exported S-parameters at 20 001
wavelengths, 1500 to 1600 nm:

| Circuit | Largest difference in power | Largest difference in field |
|---|---|---|
| All-pass ring (100 µm, κ 0.05, n_g 4.2, 3 dB/cm) | 4 × 10⁻⁹ | 2 × 10⁻⁸ |
| 1 unequal MZI stage | 2 × 10⁻¹⁰ | 6 × 10⁻¹⁰ |
| 8 stages (9 couplers, 16 waveguides) | 5 × 10⁻¹⁰ | 5 × 10⁻⁹ |
| 32 stages (33 couplers, 64 waveguides) | 1.5 × 10⁻⁹ | 2 × 10⁻⁸ |

The differences are at the rounding of the exported numbers; both keep the lossless
lattices' power at exactly 1. One convention to know when moving designs between the
two: PhotonForge's ring coupling is a field coefficient with its phase, so Shine's power
coupling κ corresponds to `kappa1 = i√κ`.

## What the models leave out

Circuit-level simulation describes each component by how it passes light, so it is only
as good as those descriptions. Shine's light runs one way (no reflections or
back-scatter), in one polarisation, at a steady state; see
[How the simulation works](simulation.md) for the full list, and the
[roadmap](roadmap.md#milestone-towards-the-chip) for what comes when a design heads for a
chip.
