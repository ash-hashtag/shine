"""Run Shine's optical neural network for handwritten digits in other tools,
and compare with Shine.

The chip's two optical layers are exported from Shine as SAX netlists
(`cargo run --release --example digits -- --export DIR --scores DIR/shine.json`).
Each tool turns a layer's netlist into its 16-input matrix at 1550 nm:

- SAX solves the netlist with the models in the exported file;
- PhotonForge rebuilds the same netlist from its own analytic models
  (directional couplers, waveguides, two-port phase and loss models)
  and solves it with its circuit model.

Then the same electronics run around each tool's layers (modulators set the
inputs, photodetectors and the neurons' clamp between the layers, the largest
of 10 outputs is the digit) on the 450 test digits, and the scores and digits
are compared with what Shine simulated for the whole chip.

    python reference/digits/compare.py DIR reference/digits/test-digits.json [--photonforge]

SAX needs `pip install sax`; PhotonForge `pip install photonforge` (its local
analytic models need no account).
"""
import importlib.util, json, sys, time
import numpy as np

C = 299_792_458.0
WL_UM = 1.55

directory, digits_file = sys.argv[1], sys.argv[2]
chip = json.load(open(f"{directory}/chip.json"))
shine = json.load(open(f"{directory}/shine.json"))
test = json.load(open(digits_file))
X = np.array(test["pixels"])
labels = np.array(test["labels"])
shine_scores = np.array([d["scores"] for d in shine["digits"]])


def load(name):
    spec = importlib.util.spec_from_file_location(name, f"{directory}/{name}.py")
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def matrix(s, inputs, outputs):
    """The layer's field from each input to each output, from an S dict."""
    return np.array([[complex(np.asarray(s(i, o)).reshape(-1)[0]) for i in inputs] for o in outputs])


def infer(m1, m2):
    """The chip's electronics around two layer matrices: scores for every digit."""
    p1 = np.abs((chip["input_field"] * X) @ m1.T) ** 2
    h = np.clip(chip["neuron_gain"] * p1, 0, 1)
    return np.abs((chip["input_field2"] * h) @ m2.T) ** 2


def report(tool, timing, scores):
    read = scores.argmax(axis=1)
    top = shine_scores.max(axis=1, keepdims=True)
    diff = np.max(np.abs(scores - shine_scores) / top)
    print(f"{tool:<12} {timing} | accuracy {np.mean(read == labels):.3f} | "
          f"same digit as Shine {np.sum(read == shine_scores.argmax(axis=1))}/{len(labels)} | "
          f"scores within {diff:.1e} of the largest")


print(f"{'Shine':<12} whole chip, {shine['seconds'] / len(labels) * 1e3:.1f} ms a digit | "
      f"accuracy {np.mean(shine_scores.argmax(axis=1) == labels):.3f}")

# ---- SAX ------------------------------------------------------------------------
t = time.perf_counter()
layers = [load("layer1"), load("layer2")]
built = time.perf_counter() - t


def sax_layers():
    mats = []
    for m in layers:
        s = m.circuit(wl=WL_UM)
        mats.append(matrix(lambda i, o: s[i, o], m.INPUTS, m.OUTPUTS))
    return mats


t = time.perf_counter()
sax_layers()
first = time.perf_counter() - t
t = time.perf_counter()
mats = sax_layers()
again = time.perf_counter() - t
report("SAX", f"netlists {built:5.2f} s, first solve {first:5.2f} s, again {again:5.3f} s", infer(*mats))

# ---- PhotonForge ----------------------------------------------------------------
if "--photonforge" not in sys.argv:
    sys.exit()
import photonforge as pf

pf.config.default_technology = pf.basic_technology()
F = C / (WL_UM * 1e-6)
cache = {}


def part(kind, settings):
    """A PhotonForge component for one netlist instance: ports in0, in1…,
    out0, out1…, as in Shine's models."""
    key = (kind, tuple(sorted(settings.items())))
    if key in cache:
        return cache[key]
    c = pf.Component(f"{kind}{len(cache)}")
    if kind == "coupler":
        # 50:50 (all the mesh's couplers): coupling length twice the interaction length.
        assert abs(settings["coupling"] - 0.5) < 1e-12
        ports = ["in0", "in1", "out0", "out1"]
        for name, (x, y, a) in zip(ports, [(0, 0, 0), (0, 2, 0), (10, 0, 180), (10, 2, 180)]):
            c.add_port(pf.Port((x, y), a, "Strip"), name)
        c.add_model(pf.AnalyticDirectionalCouplerModel(interaction_length=1.0, coupling_length=2.0, propagation_length=0.0,
                                                        n_eff=2.4, n_group=4.2, reference_frequency=F, ports=ports), "dc")
    else:
        c.add_port(pf.Port((0, 0), 0, "Strip"), "in0")
        c.add_port(pf.Port((10, 0), 180, "Strip"), "out0")
        if kind == "waveguide":
            c.add_model(pf.AnalyticWaveguideModel(n_eff=settings["n_eff"], n_group=settings["n_g"], length=settings["length"],
                                                  propagation_loss=settings["loss_db_cm"] * 1e-4,
                                                  reference_frequency=C / (settings["wl0"] * 1e-6)), "wg")
        elif kind == "phase_shifter":
            c.add_model(pf.TwoPortModel(t=np.exp(1j * settings["phase"]), ports=["in0", "out0"]), "ps")
        elif kind == "attenuator":
            c.add_model(pf.TwoPortModel(t=settings["amplitude"], ports=["in0", "out0"]), "att")
        else:
            raise ValueError(f"no PhotonForge part for {kind}")
    cache[key] = c
    return c


def photonforge_layer(m, name):
    net = m.netlist
    top = pf.Component(name)
    refs = {}
    for k, (inst, spec) in enumerate(net["instances"].items()):
        # Apart, so no ports touch: every connection is the netlist's (virtual).
        refs[inst] = pf.Reference(part(spec["component"], spec.get("settings", {})), origin=(20 * (k % 100), 20 * (k // 100)))
        top.add(refs[inst])
    for a, b in net["connections"].items():
        (ia, pa), (ib, pb) = a.split(","), b.split(",")
        top.add_virtual_connection(refs[ia], pa, refs[ib], pb)
    for port, at in net["ports"].items():
        inst, p = at.split(",")
        top.add_port(refs[inst][p], port)
    top.add_model(pf.CircuitModel(), "circuit")
    return top


t = time.perf_counter()
circuits = [photonforge_layer(m, f"layer{k + 1}") for k, m in enumerate(layers)]
built = time.perf_counter() - t


def photonforge_layers():
    mats = []
    for m, c in zip(layers, circuits):
        s = c.s_matrix([F], show_progress=False).elements
        mats.append(matrix(lambda i, o: s[(f"{i}@0", f"{o}@0")], m.INPUTS, m.OUTPUTS))
    return mats


t = time.perf_counter()
photonforge_layers()
first = time.perf_counter() - t
t = time.perf_counter()
mats = photonforge_layers()
again = time.perf_counter() - t
report("PhotonForge", f"circuits {built:5.2f} s, first solve {first:5.2f} s, again {again:5.3f} s", infer(*mats))
