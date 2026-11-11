//! A netlist for [SAX](https://github.com/flaport/sax), the circuit
//! simulator of the gdsfactory family: a Python file with the design's
//! instances, connections and ports, and a model for each kind of component
//! that computes exactly what Shine does. SAX runs it as it is; swap a model
//! for your foundry kit's (`models["coupler"] = pdk.coupler`) to see the
//! design with real components, or fit it into a larger SAX circuit.
//!
//! The netlist is of the circuit Shine simulates: components inside a design
//! are flattened, rings and trees are their couplers and waveguides, and
//! signals inside (heaters, switches) are at the values they settle to.
//! Components known only by their S-parameters (imported or measured) go in
//! as tables, interpolated as Shine does.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use super::gdsfactory::ident;
use crate::panels::circuit::sim::{self, Machine};
use crate::photonics::{C64, Kind, Matrix, NodeId};
use crate::panels::project::{DocId, Project, layout_interface};

/// The result: the Python text, and what's in it.
#[derive(Clone, Debug, PartialEq)]
pub struct SaxExport {
    pub python: String,
    pub instances: usize,
    pub connections: usize,
    pub ports: usize,
}

/// The models every netlist can use: functions of the wavelength `wl` in
/// µm, as SAX takes it, written to match Shine's own.
const MODELS: &str = r#"def _sdict(entries, wl):
    """A reciprocal S-dictionary from {(input, output): field}, each as an
    array over the wavelengths."""
    one = jnp.ones_like(jnp.asarray(wl, dtype=float))
    return sax.reciprocal({k: v * one for k, v in entries.items()})


def waveguide(wl=1.55, length=10.0, n_eff=2.4, n_g=2.4, wl0=1.55, loss_db_cm=0.0):
    """A straight waveguide: index n_eff at wl0, to first order around it
    with group index n_g; loss in dB/cm. Lengths in µm."""
    wl = jnp.asarray(wl, dtype=float)
    n = n_eff - (n_g - n_eff) * (wl - wl0) / wl0
    amplitude = 10 ** (-loss_db_cm * length * 1e-4 / 20)
    return _sdict({("in0", "out0"): amplitude * jnp.exp(2j * jnp.pi * n * length / wl)}, wl)


def coupler(wl=1.55, coupling=0.5):
    """Lossless 2×2 coupler: `coupling` of the power crosses, a quarter
    turn ahead."""
    t, x = jnp.sqrt(1 - coupling), 1j * jnp.sqrt(coupling)
    return _sdict({("in0", "out0"): t, ("in0", "out1"): x, ("in1", "out0"): x, ("in1", "out1"): t}, wl)


def phase_shifter(wl=1.55, phase=0.0):
    """A fixed phase in radians, as a heater sets."""
    return _sdict({("in0", "out0"): jnp.exp(1j * phase)}, wl)


def attenuator(wl=1.55, amplitude=1.0):
    """A wavelength-independent loss; `amplitude` is the field kept."""
    return _sdict({("in0", "out0"): amplitude}, wl)


def mzi(wl=1.55, theta=0.0, phi=0.0):
    """A Mach–Zehnder switch: phase phi on input 0, then θ between two 50:50
    couplers (θ = 0 crosses, θ = π is the bar state)."""
    g, e = 1j * jnp.exp(0.5j * theta), jnp.exp(1j * phi)
    s, c = jnp.sin(theta / 2), jnp.cos(theta / 2)
    return _sdict({("in0", "out0"): g * e * s, ("in1", "out0"): g * c, ("in0", "out1"): g * e * c, ("in1", "out1"): -g * s}, wl)


def y_splitter(wl=1.55):
    """A 1×2 Y-junction: half the power each way."""
    a = jnp.sqrt(0.5)
    return _sdict({("in0", "out0"): a, ("in0", "out1"): a}, wl)


def y_combiner(wl=1.55):
    """A 2×1 Y-junction: the in-phase half is guided, the rest radiates."""
    a = jnp.sqrt(0.5)
    return _sdict({("in0", "out0"): a, ("in1", "out0"): a}, wl)


def crossing(wl=1.55):
    """A waveguide crossing: each input goes straight across."""
    return _sdict({("in0", "out1"): 1.0, ("in1", "out0"): 1.0}, wl)


def mirror(wl=1.55, reflectivity=1.0):
    """A mirror: sends back √R of the field, with a half turn."""
    return _sdict({("in0", "out0"): -jnp.sqrt(reflectivity)}, wl)


def table(wavelengths_um, matrices):
    """A model from S-matrices (`outputs × inputs`) at ascending
    wavelengths: magnitude and unwrapped phase interpolated linearly,
    the ends held beyond them."""
    xs = np.asarray(wavelengths_um, dtype=float)
    m = np.asarray(matrices, dtype=complex)
    magnitude, phase = np.abs(m), np.unwrap(np.angle(m), axis=0)
    outputs, inputs = m.shape[1:]

    def model(wl=1.55):
        wl = jnp.asarray(wl, dtype=float)
        return _sdict(
            {
                (f"in{i}", f"out{o}"): jnp.interp(wl, xs, magnitude[:, o, i]) * jnp.exp(1j * jnp.interp(wl, xs, phase[:, o, i]))
                for o in range(outputs)
                for i in range(inputs)
                if magnitude[:, o, i].any()
            },
            wl,
        )

    return model


models = {
    "waveguide": waveguide,
    "coupler": coupler,
    "phase_shifter": phase_shifter,
    "attenuator": attenuator,
    "mzi": mzi,
    "y_splitter": y_splitter,
    "y_combiner": y_combiner,
    "crossing": crossing,
    "mirror": mirror,
}
"#;

/// A Python float.
fn float(v: f64) -> String {
    if v.is_finite() {
        format!("{v:?}")
    } else {
        "float('nan')".into()
    }
}

fn complex(z: C64) -> String {
    format!("complex({}, {})", float(z.re), float(z.im))
}

fn matrix(m: &Matrix) -> String {
    let rows: Vec<String> = (0..m.nrows()).map(|o| format!("[{}]", (0..m.ncols()).map(|i| complex(m[(o, i)])).collect::<Vec<_>>().join(", "))).collect();
    format!("[{}]", rows.join(", "))
}

/// Wavelengths in µm, as Python: a `linspace` when evenly spaced.
fn wavelength_list(nm: &[f64]) -> String {
    let n = nm.len();
    let even = n > 2 && nm.windows(2).all(|w| ((w[1] - w[0]) - (nm[1] - nm[0])).abs() < 1e-9 * nm[0].abs().max(1.0));
    if even {
        format!("np.linspace({}, {}, {n}).tolist()", float(nm[0] / 1000.0), float(nm[n - 1] / 1000.0))
    } else {
        format!("[{}]", nm.iter().map(|w| float(w / 1000.0)).collect::<Vec<_>>().join(", "))
    }
}

/// A Python string literal.
fn string(text: &str) -> String {
    format!("{text:?}")
}

/// The SAX netlist of document `doc`. Components with no model of their own
/// are sampled at `wavelengths_nm`, which is also what the file prints when
/// run. `source` names the project file, for the header.
pub fn export(project: &Project, doc: DocId, wavelengths_nm: &[f64], source: &str) -> Result<SaxExport, String> {
    let d = project.doc(doc).ok_or("no such document")?;
    let (circuit, ports) = sim::circuit(d.layout(), d.panel().cell_um, project, &Machine::default(), Some(doc))?;
    if ports.inputs.is_empty() || ports.outputs.is_empty() {
        return Err("It needs light Input and Output ports (or pins) for a netlist".into());
    }
    // Sources and sinks aren't components in SAX: the netlist's ports are
    // where light goes in and comes out.
    let kept: Vec<NodeId> = circuit
        .nodes()
        .filter(|&n| {
            let c = circuit.component(n);
            !matches!(c.kind(), Kind::Laser | Kind::Detector) && c.num_inputs() > 0 && c.num_outputs() > 0
        })
        .collect();
    let kept_set: HashSet<NodeId> = kept.iter().copied().collect();

    let mut instances = String::new();
    let mut tables = String::new();
    let mut names: HashMap<NodeId, String> = HashMap::new();
    for &n in &kept {
        let c = circuit.component(n);
        let (component, settings): (String, Vec<(&str, f64)>) = match c.kind() {
            Kind::Waveguide(w) => (
                "waveguide".into(),
                vec![("length", w.length_um), ("n_eff", w.n_eff), ("n_g", w.n_g), ("wl0", w.reference_nm / 1000.0), ("loss_db_cm", w.loss_db_per_cm)],
            ),
            Kind::PhaseShifter { phase } => ("phase_shifter".into(), vec![("phase", phase)]),
            Kind::Attenuator { amplitude } => ("attenuator".into(), vec![("amplitude", amplitude)]),
            Kind::Coupler { coupling } => ("coupler".into(), vec![("coupling", coupling.clamp(0.0, 1.0))]),
            Kind::Mzi { theta, phi } => ("mzi".into(), vec![("theta", theta), ("phi", phi)]),
            Kind::YSplitter => ("y_splitter".into(), Vec::new()),
            Kind::YCombiner => ("y_combiner".into(), Vec::new()),
            Kind::Crossing => ("crossing".into(), Vec::new()),
            Kind::Mirror { reflectivity } => ("mirror".into(), vec![("reflectivity", reflectivity.clamp(0.0, 1.0))]),
            kind => {
                // Known by its matrices: a table model of its own.
                let model = format!("table_{}", n.index());
                let (xs, ms): (Vec<f64>, Vec<Matrix>) = match kind {
                    Kind::Constant(m) => (vec![1.55], vec![m.clone()]),
                    Kind::Table(t) => (t.wavelengths_nm.iter().map(|w| w / 1000.0).collect(), t.matrices.clone()),
                    _ => (wavelengths_nm.iter().map(|w| w / 1000.0).collect(), wavelengths_nm.iter().map(|&w| c.s_matrix(w)).collect()),
                };
                let _ = writeln!(tables, "# {}", c.name());
                let _ = writeln!(
                    tables,
                    "models[{}] = table(\n    [{}],\n    [{}],\n)",
                    string(&model),
                    xs.iter().map(|&x| float(x)).collect::<Vec<_>>().join(", "),
                    ms.iter().map(matrix).collect::<Vec<_>>().join(",\n     ")
                );
                (model, Vec::new())
            }
        };
        // Tables are named for their node already.
        let name = if component.starts_with("table_") { component.clone() } else { format!("{component}_{}", n.index()) };
        let _ = write!(instances, "        {}: {{\"component\": {}", string(&name), string(&component));
        if !settings.is_empty() {
            let s: Vec<String> = settings.iter().map(|(k, v)| format!("{}: {}", string(k), float(*v))).collect();
            let _ = write!(instances, ", \"settings\": {{{}}}", s.join(", "));
        }
        let _ = writeln!(instances, "}},");
        names.insert(n, name);
    }

    let mut connections = String::new();
    let mut links = 0;
    for &n in &kept {
        for k in 0..circuit.component(n).num_outputs() {
            if let Some(to) = circuit.sink_of(n.output(k)).filter(|to| kept_set.contains(&to.node)) {
                let _ = writeln!(connections, "        \"{},out{k}\": \"{},in{}\",", names[&n], names[&to.node], to.index);
                links += 1;
            }
        }
    }

    // The ports, named as the design names its light inputs and outputs.
    let iface = layout_interface(d.layout());
    let mut taken: HashSet<String> = HashSet::new();
    let mut unique = |name: &str| {
        let base = ident(name);
        let mut n = base.clone();
        let mut k = 2;
        while !taken.insert(n.clone()) {
            n = format!("{base}_{k}");
            k += 1;
        }
        n
    };
    let ins: Vec<(String, String)> = ports
        .inputs
        .iter()
        .enumerate()
        .filter(|(_, p)| kept_set.contains(&p.node))
        .map(|(k, p)| (unique(iface.inputs.get(k).map_or("in", String::as_str)), format!("{},in{}", names[&p.node], p.index)))
        .collect();
    let outs: Vec<(String, String)> = ports
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, p)| kept_set.contains(&p.node))
        .map(|(k, p)| (unique(iface.outputs.get(k).map_or("out", String::as_str)), format!("{},out{}", names[&p.node], p.index)))
        .collect();

    let mut py = String::new();
    let _ = writeln!(py, "# “{}” from Shine {}{}.", d.name, env!("CARGO_PKG_VERSION"), if source.is_empty() { String::new() } else { format!(" ({source})") });
    let _ = writeln!(py, "# A netlist for SAX (https://github.com/flaport/sax), with models that compute what");
    let _ = writeln!(py, "# Shine does. Run it (pip install sax) to print the transmission between the ports,");
    let _ = writeln!(py, "# or import it and swap models for your foundry kit's:");
    let _ = writeln!(py, "#");
    let _ = writeln!(py, "#   models[\"coupler\"] = my_pdk.coupler");
    let _ = writeln!(py, "#   circuit, _ = sax.circuit(netlist=netlist, models=models)");
    let _ = writeln!(py, "#   s = circuit(wl=jnp.linspace(1.5, 1.6, 1001))  # wavelengths in µm");
    let _ = writeln!(py, "#");
    let _ = writeln!(py, "# Light is one-way in Shine: the models are reciprocal and reflect nothing.");
    let _ = writeln!(py, "\nimport jax.numpy as jnp\nimport numpy as np\nimport sax\n\n");
    py.push_str(MODELS);
    if !tables.is_empty() {
        let _ = writeln!(py, "\n# Components known by their S-parameters (wavelengths in µm).");
        py.push_str(&tables);
    }
    let _ = writeln!(py, "\nnetlist = {{");
    let _ = writeln!(py, "    \"instances\": {{\n{instances}    }},");
    let _ = writeln!(py, "    \"connections\": {{\n{connections}    }},");
    let _ = writeln!(py, "    \"ports\": {{");
    for (name, end) in ins.iter().chain(&outs) {
        let _ = writeln!(py, "        {}: {},", string(name), string(end));
    }
    let _ = writeln!(py, "    }},\n}}");
    let _ = writeln!(py, "\nINPUTS = [{}]", ins.iter().map(|(n, _)| string(n)).collect::<Vec<_>>().join(", "));
    let _ = writeln!(py, "OUTPUTS = [{}]", outs.iter().map(|(n, _)| string(n)).collect::<Vec<_>>().join(", "));
    let _ = writeln!(py, "WAVELENGTHS_UM = {}", wavelength_list(wavelengths_nm));
    py.push_str(
        r#"

circuit, _ = sax.circuit(netlist=netlist, models=models)

if __name__ == "__main__":
    s = circuit(wl=jnp.asarray(WAVELENGTHS_UM))
    print("wavelength_nm," + ",".join(f"{i}->{o}" for i in INPUTS for o in OUTPUTS))
    for k, wl in enumerate(WAVELENGTHS_UM):
        powers = (float(jnp.abs(jnp.atleast_1d(s[i, o])[k]) ** 2) for i in INPUTS for o in OUTPUTS)
        print(f"{wl * 1000:g}," + ",".join(f"{p:.9g}" for p in powers))
"#,
    );
    Ok(SaxExport { python: py, instances: kept.len(), connections: links, ports: ins.len() + outs.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::optics::model::{BlockRef, Builtin, Layout, PortRef};

    #[test]
    fn a_ring_becomes_a_coupler_and_its_loop() {
        let mut p = Project::new();
        let mut l = Layout::default();
        let pin = l.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "in".into() }), (0, 0));
        let ring = l.add_instance(
            BlockRef::Builtin(Builtin::Ring { coupling: 0.1, length_um: 100.0, n_eff: 2.4, loss_db_per_cm: 3.0, n_g: 4.2, reference_nm: 1550.0 }),
            (3, 0),
        );
        let out = l.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "through".into() }), (7, 0));
        l.connect_optical(PortRef { instance: pin, port: 0 }, PortRef { instance: ring, port: 0 });
        l.connect_optical(PortRef { instance: ring, port: 0 }, PortRef { instance: out, port: 0 });
        let id = p.add_layout("Ring", l, 10.0);
        let e = export(&p, id, &[1550.0], "r.shine").unwrap();
        let py = &e.python;
        assert!(py.contains("\"component\": \"coupler\", \"settings\": {\"coupling\": 0.1}"), "{py}");
        assert!(py.contains("\"length\": 100.0, \"n_eff\": 2.4, \"n_g\": 4.2, \"wl0\": 1.55, \"loss_db_cm\": 3.0"), "{py}");
        assert!(py.contains("\"in\": ") && py.contains("\"through\": "), "{py}");
        assert_eq!(e.ports, 2);
        // The loop: the coupler's output back round to its input.
        assert!(e.connections >= 2, "{py}");
    }

    #[test]
    fn imported_s_parameters_become_tables() {
        let mut p = Project::new();
        let mut l = Layout::default();
        let pin = l.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "a".into() }), (0, 0));
        use crate::panels::models::{ModelSource, ModelSpec};
        let source = ModelSource::Table { origin: "test".into(), wavelengths_nm: vec![1500.0, 1600.0], values: vec![vec![[0.9, 0.0]], vec![[0.0, 0.8]]] };
        let spec = ModelSpec::new("Measured", vec!["a".into()], vec!["b".into()], source);
        let m = l.add_instance(BlockRef::Builtin(Builtin::Model(std::sync::Arc::new(spec))), (3, 0));
        let out = l.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "b".into() }), (6, 0));
        l.connect_optical(PortRef { instance: pin, port: 0 }, PortRef { instance: m, port: 0 });
        l.connect_optical(PortRef { instance: m, port: 0 }, PortRef { instance: out, port: 0 });
        let id = p.add_layout("Measured", l, 10.0);
        let py = export(&p, id, &[1550.0], "").unwrap().python;
        assert!(py.contains("= table(\n    [1.5, 1.6],"), "{py}");
    }

    #[test]
    fn no_ports_no_netlist() {
        let mut p = Project::new();
        let id = p.add_layout("Empty", Layout::default(), 10.0);
        assert!(export(&p, id, &[1550.0], "").unwrap_err().contains("Input and Output"));
    }
}
