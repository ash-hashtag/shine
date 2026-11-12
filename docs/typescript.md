---
title: TypeScript
description: Design, simulate and analyse photonic circuits from TypeScript or JavaScript, in Node or the browser, with the shine-photonics package.
order: 13.2
---

# TypeScript

The `shine-photonics` npm package is Shine as a library for TypeScript and JavaScript, in
Node.js and in the browser: build components and designs, simulate, sweep and analyse them,
and export them, with no window. It is Shine's engine compiled to WebAssembly, with types.
What you build is a normal Shine project that opens in the app.

```sh
npm install shine-photonics
```

```typescript
import { Project } from "shine-photonics";

const project = Project.new();
const chip = project.newDesign("Ring filter");

const laser = chip.laser(1550, 1);           // wavelengthNm, powerMw
const ring = chip.ring(0.05, 100);           // coupling, lengthUm
const detector = chip.detector();
laser.to(ring).to(detector);                 // light flows laser → ring → detector

console.log(chip.simulate().powerMw(detector));       // 0.9987…

const spectrum = chip.sweep(laser, "wavelength", 1540, 1560, 2001);
console.log(spectrum.readouts("Detector").fsrNm);     // 5.72 nm

const file: Uint8Array = project.toBytes();           // a .shine file
```

The API is the [Python one](python.md) with camelCase names: `newDesign`, `powerMw`,
`lightInput`, `findPart`, `newModelFromFunction`. Optional arguments come last
(`chip.ring(0.05, 100, 2.4, 4.2, 3)` sets n_eff, n_g and loss), and every parameter can be
set by name: `ring.set("loss", 3)`.

## Components written in TypeScript

```typescript
const filter = {
  matrix(wavelengthNm: number) {             // rows: outputs, columns: inputs
    const a = Math.min(1, Math.abs(wavelengthNm - 1560) / 10);
    return { rows: 1, cols: 1, values: [{ re: a, im: 0 }] };
  },
};
const f = project.newModelFromFunction("Filter", ["in"], ["out"], [1550, 1555, 1560], filter);
chip.laser(1555, 1).to(chip.place(f)).to(chip.detector());
```

Shine samples the function at the wavelengths given and keeps the table, so the component
saves with the project and opens in the app.

## Errors

Every failure throws `ShineErrorException`, whose message says what would have worked:

```typescript
try {
  ring.port("drop");
} catch (e) {
  console.log(e.message);   // “Ring resonator” has no port “drop”; it has: in, through
}
```

## In the browser and in Node

- In Node, `Project.open(path)` and `project.save(path)` read and write `.shine` files; in
  the browser, use `Project.fromBytes(data)` and `project.toBytes()`, and open the file in
  the app at [shine.lupyd.com/app](https://shine.lupyd.com/app) with File → Open.
- What needs the desktop isn't in the WebAssembly build: optimisation, Touchstone import,
  scripted components and `show()` throw an error that says so. Everything else, including
  sweeps and Monte Carlo, works the same.
- Objects are freed when garbage collected; call `dispose()` to free one sooner.
