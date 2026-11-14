"""The Python package, as the design spec shows it (design/library-api.md).

    python -m unittest discover -s bindings/tests
"""
import math
import os
import tempfile
import unittest

import shine_photonics as shine


def modulator_link():
    """A modulator written as a component, used on a chip."""
    project = shine.Project()
    mzm = project.new_component("Modulator")
    light_in = mzm.light_input("in")
    light_out = mzm.light_output("out")
    split = mzm.y_splitter()
    heater = mzm.phase_shifter(phase=math.pi)
    upper = mzm.waveguide(length_um=100)
    lower = mzm.waveguide(length_um=100)
    join = mzm.y_combiner()
    light_in.to(split).to(heater).to(upper)
    mzm.connect(split.port("out2"), lower.port("in"))
    mzm.connect(upper.port("out"), join.port("in1"))
    mzm.connect(lower.port("out"), join.port("in2"))
    join.to(light_out)
    mzm.expose("bias", heater, "phase")

    chip = project.new_design("Link")
    laser = chip.laser(wavelength_nm=1550, power_mw=1)
    modulator = chip.place(mzm)
    detector = chip.detector()
    laser.to(modulator).to(detector)
    return project, chip, modulator, detector


class Library(unittest.TestCase):
    def test_a_component_written_in_code_works_on_a_chip(self):
        _, chip, modulator, detector = modulator_link()
        self.assertLess(chip.simulate().power_mw(detector), 1e-12, "the heater starts at π: the arms cancel")
        modulator.set("bias", 0.0)
        self.assertAlmostEqual(chip.simulate().power_mw(detector), 1.0, places=12)
        sweep = chip.sweep(modulator, "bias", 0, math.pi, points=5)
        for x, y in zip(sweep.x(), sweep.series("Detector").power_mw):
            self.assertAlmostEqual(y, math.cos(x / 2) ** 2, places=12)
        self.assertEqual(sweep.x_label(), "bias (rad)")

    def test_parts_explain_themselves_and_errors_say_what_would_work(self):
        chip = shine.Project().new_design("Filter")
        ring = chip.ring(coupling=0.05, length_um=100, loss_db_per_cm=3)
        self.assertEqual((ring.name(), ring.kind()), ("Ring resonator", "Ring resonator"))
        self.assertEqual([p.name for p in ring.ports()], ["in", "through"])
        self.assertEqual(ring.ports()[1].direction, shine.Direction.OUT)
        self.assertEqual(ring.get("loss"), 3)
        self.assertIn("n_g", [p.name for p in ring.params()])
        with self.assertRaises(shine.ShineErrorException) as e:
            ring.port("drop")
        self.assertEqual(str(e.exception), "“Ring resonator” has no port “drop”; it has: in, through")
        with self.assertRaisesRegex(shine.ShineErrorException, "fraction from 0 to 1"):
            chip.coupler(1.5)

    def test_a_rings_spectrum_has_its_textbook_spacing(self):
        chip = shine.Project().new_design("Ring")
        laser = chip.laser()
        ring = chip.ring(0.05, 100)
        detector = chip.detector()
        laser.to(ring).to(detector)
        r = chip.sweep(laser, "wavelength", 1540, 1560, points=4001).readouts("Detector")
        fsr = 1550**2 / (4.2 * 100 * 1000)
        self.assertTrue(r.dips)
        self.assertAlmostEqual(r.fsr_nm, fsr, delta=0.01 * fsr)

    def test_transmission_and_files(self):
        project, chip, modulator, detector = modulator_link()
        mzm = project.design("Modulator")
        t = mzm.transmission([1550.0])
        self.assertEqual((t.inputs(), t.outputs()), (["in"], ["out"]))
        self.assertLess(t.power("out", "in")[0], 1e-12)
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "link.shine")
            modulator.set("bias", 0.0)
            project.save(path)
            again = shine.Project.open(path)
        self.assertEqual(again.designs(), ["Modulator", "Link"])
        link = again.design("Link")
        self.assertAlmostEqual(link.simulate().power_mw(link.find_part("Detector")), 1.0, places=12)
        self.assertIn("pdk", chip.to_gdsfactory().lower())
        self.assertIn("sax.circuit", mzm.to_sax([1550.0]))

    def test_a_component_written_as_a_python_function(self):
        class Filter:
            """Passes 1550 nm, blocks 1560 nm."""

            def matrix(self, wavelength_nm):
                a = min(1.0, abs(wavelength_nm - 1560) / 10)
                return shine.Matrix(rows=1, cols=1, values=[shine.Complex(re=a, im=0.0)])

        project = shine.Project()
        f = project.new_model_from_function("Filter", ["in"], ["out"], [1550.0, 1555.0, 1560.0], Filter())
        chip = project.new_design("Test")
        laser = chip.laser(wavelength_nm=1555)
        pd = chip.detector()
        laser.to(chip.place(f)).to(pd)
        self.assertAlmostEqual(chip.simulate().power_mw(pd), 0.25, places=12)

    def test_several_aims_across_a_band(self):
        project = shine.Project()
        chip = project.new_design("Split")
        laser, dc = chip.laser(), chip.coupler(0.5)
        bar, cross = chip.detector(), chip.detector()
        laser.to(dc)
        chip.connect(dc.port("out1"), bar.port("in"))
        chip.connect(dc.port("out2"), cross.port("in"))
        aims = [
            shine.Aim(output=bar.name(), goal=shine.Goal.TARGET, target_mw=0.25, band_from_nm=1540.0, band_to_nm=1560.0, band_points=5, weight=1.0),
            shine.Aim(output=cross.name(), goal=shine.Goal.TARGET, target_mw=0.75, band_from_nm=0.0, band_to_nm=0.0, band_points=0, weight=1.0),
        ]
        r = chip.optimise_aims([shine.Variable(part=dc.name(), param="coupling", min=0.0, max=1.0)], aims, 200)
        self.assertAlmostEqual(r.values[0], 0.75, places=3)
        self.assertAlmostEqual(r.after_mw[1], 0.75, places=5)

    def test_a_weight_matrix_becomes_a_layer_of_light(self):
        rows = [[0.5, -1.0, 2.0], [0.25, 0.0, 1.0]]
        w = shine.Matrix(rows=2, cols=3, values=[shine.Complex(re=x, im=0.0) for row in rows for x in row])
        project = shine.Project()
        layer = project.new_weight_layer("Weights", w)
        self.assertIn(f"{shine.layer_scale(w):.6f}", layer.description())
        t = layer.transmission([1550.0])
        self.assertEqual((t.inputs(), t.outputs()), (["in1", "in2", "in3"], ["out1", "out2"]))
        # Powers in the ratios of |W|².
        self.assertAlmostEqual(t.power("out1", "in3")[0] / t.power("out1", "in1")[0], 16.0, places=9)
        with self.assertRaisesRegex(shine.ShineErrorException, "needs 6 values"):
            project.new_weight_layer("Bad", shine.Matrix(rows=2, cols=3, values=[]))

    def test_monte_carlo_and_optimisation(self):
        chip = shine.Project().new_design("Split")
        laser = chip.laser()
        dc = chip.coupler(0.5)
        pd = chip.detector()
        laser.to(dc).to(pd)
        a = chip.monte_carlo(20, seed=7, variation=shine.Variation())
        b = chip.monte_carlo(20, seed=7, variation=None)
        self.assertEqual(a.values("Detector"), b.values("Detector"), "the defaults, from the same seed")
        self.assertAlmostEqual(a.stats("Detector").mean, 0.5, delta=0.05)
        coupling = [shine.Variable(part="Directional coupler", param="coupling", min=0, max=1)]
        r = chip.optimise(coupling, "Detector", shine.Goal.TARGET, target_mw=0.2)
        self.assertAlmostEqual(r.values[0], 0.8, delta=1e-3)
        chip.apply(coupling, r.values)
        self.assertAlmostEqual(dc.get("coupling"), 0.8, delta=1e-3)

    def test_electronics_drive_the_light(self):
        chip = shine.Project().new_design("Switch")
        laser = chip.laser()
        att = chip.attenuator(loss_db=0)
        pd = chip.detector()
        value = chip.value_input(0.25)
        laser.to(att).to(pd)
        chip.connect(value.port("v"), att.port("transmission"))
        self.assertAlmostEqual(chip.simulate().power_mw(pd), 0.25, places=12)
        n, d = chip.number_input(bits=4, value=9), chip.display(bits=4)
        for b in ["b3", "b2", "b1", "b0"]:
            chip.connect(n.port(b), d.port(b))
        self.assertEqual(chip.simulate().display(d), 9)


if __name__ == "__main__":
    unittest.main()
