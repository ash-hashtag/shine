// The TypeScript package, as the design spec shows it (design/library-api.md).
//   cd bindings && pnpm test
import { test } from "node:test";
import assert from "node:assert/strict";
import { Direction, Goal, Project, ShineErrorException } from "../dist/wasm/pkg/node.js";

const close = (a, b, tol, what) => assert.ok(Math.abs(a - b) <= tol, `${what ?? ""} ${a} vs ${b}`);

function modulatorLink() {
  const project = Project.new();
  const mzm = project.newComponent("Modulator");
  const lightIn = mzm.lightInput("in");
  const lightOut = mzm.lightOutput("out");
  const split = mzm.ySplitter();
  const heater = mzm.phaseShifter(Math.PI);
  const upper = mzm.waveguide(100);
  const lower = mzm.waveguide(100);
  const join = mzm.yCombiner();
  lightIn.to(split).to(heater).to(upper);
  mzm.connect(split.port("out2"), lower.port("in"));
  mzm.connect(upper.port("out"), join.port("in1"));
  mzm.connect(lower.port("out"), join.port("in2"));
  join.to(lightOut);
  mzm.expose("bias", heater, "phase");
  const chip = project.newDesign("Link");
  const laser = chip.laser(1550, 1);
  const modulator = chip.place(mzm);
  const detector = chip.detector();
  laser.to(modulator).to(detector);
  return { project, chip, modulator, detector };
}

test("a component written in code works on a chip", () => {
  const { chip, modulator, detector } = modulatorLink();
  assert.ok(chip.simulate().powerMw(detector) < 1e-12);
  modulator.set("bias", 0);
  close(chip.simulate().powerMw(detector), 1, 1e-12, "in phase");
  const sweep = chip.sweep(modulator, "bias", 0, Math.PI, 5);
  sweep.series("Detector").powerMw.forEach((y, k) => close(y, Math.cos(sweep.x()[k] / 2) ** 2, 1e-12, "cos²"));
});

test("parts explain themselves and errors say what would work", () => {
  const chip = Project.new().newDesign("Filter");
  const ring = chip.ring(0.05, 100);
  assert.deepEqual(ring.ports().map((p) => p.name), ["in", "through"]);
  assert.equal(ring.ports()[1].direction, Direction.Out);
  assert.throws(() => ring.port("drop"), (e) => e instanceof ShineErrorException && e.message === "“Ring resonator” has no port “drop”; it has: in, through");
});

test("a ring's spectrum has its textbook spacing", () => {
  const chip = Project.new().newDesign("Ring");
  const laser = chip.laser();
  const detector = chip.detector();
  laser.to(chip.ring(0.05, 100)).to(detector);
  const r = chip.sweep(laser, "wavelength", 1540, 1560, 4001).readouts("Detector");
  const fsr = 1550 ** 2 / (4.2 * 100 * 1000);
  assert.ok(r.dips);
  close(r.fsrNm, fsr, 0.01 * fsr, "FSR");
});

test("files round trip", () => {
  const { project, modulator } = modulatorLink();
  modulator.set("bias", 0);
  const again = Project.fromBytes(project.toBytes());
  assert.deepEqual(again.designs(), ["Modulator", "Link"]);
  const link = again.design("Link");
  close(link.simulate().powerMw(link.findPart("Detector")), 1, 1e-12, "the same circuit");
});

test("a component written as a TypeScript function", () => {
  const project = Project.new();
  const filter = {
    matrix(wavelengthNm) {
      const a = Math.min(1, Math.abs(wavelengthNm - 1560) / 10);
      return { rows: 1, cols: 1, values: [{ re: a, im: 0 }] };
    },
  };
  const f = project.newModelFromFunction("Filter", ["in"], ["out"], [1550, 1555, 1560], filter);
  const chip = project.newDesign("Test");
  const pd = chip.detector();
  chip.laser(1555, 1).to(chip.place(f)).to(pd);
  close(chip.simulate().powerMw(pd), 0.25, 1e-12, "amplitude 0.5");
});

test("Monte Carlo repeats from its seed", () => {
  const chip = Project.new().newDesign("Split");
  const pd = chip.detector();
  chip.laser().to(chip.coupler(0.5)).to(pd);
  const a = chip.monteCarlo(20, 7);
  const b = chip.monteCarlo(20, 7);
  assert.deepEqual(a.values("Detector"), b.values("Detector"));
  close(a.stats("Detector").mean, 0.5, 0.05, "mean");
  // Optimisation runs in the desktop library, not WebAssembly.
  assert.throws(() => chip.optimise([{ part: "Directional coupler", param: "coupling", min: 0, max: 1 }], "Detector", Goal.Target, 0.2, 100), /desktop/);
});
