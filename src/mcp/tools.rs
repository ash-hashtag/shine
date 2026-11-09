//! MCP tools: what an AI client can do with Shine.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::spec::{self, CircuitSpec};
use crate::panels::circuit::sim::{Machine, bits_to_number};
use crate::panels::optics::model::{Item, OpticComponentKind, Terminal, cell_of};
use crate::panels::project::{DocId, Project, v1};
use crate::panels::schematic::model::{BlockRef, Builtin, PortRef};
use crate::panels::schematic::sim::{self as schematic_sim, Resolver};
use crate::photonics::{C64, Circuit, InPort, Matrix, OutPort, Ports, SvdMesh, UnitaryMesh};

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    pub call: fn(&Map<String, Value>) -> Result<Value, String>,
}

fn circuit_schema() -> Value {
    json!({
        "type": "object",
        "description": "Circuit: blocks (see list_blocks) and connections from an output port to an input port, written \"block_id.port\" (port name or 0-based index). Each port takes one connection.",
        "properties": {
            "blocks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "type": {"type": "string", "description": "Block type from list_blocks, e.g. laser, coupler, mzi, detector"},
                        "params": {"type": "object", "description": "Parameters to override; others keep their defaults"}
                    },
                    "required": ["id", "type"]
                }
            },
            "connections": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "from": {"type": "string", "description": "Output port, e.g. \"dc.out2\""},
                        "to": {"type": "string", "description": "Input port, e.g. \"pd.in\""},
                        "length_um": {"type": "number", "description": "Waveguide length; omit for an ideal (phase-free) waveguide"}
                    },
                    "required": ["from", "to"]
                }
            },
            "n_eff": {"type": "number", "description": "Effective index of waveguides with a length (default 2.4)"}
        },
        "required": ["blocks"]
    })
}

fn object<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    args.get(key)
        .ok_or_else(|| format!("missing argument “{key}”"))
}

fn number(args: &Map<String, Value>, key: &str) -> Result<f64, String> {
    object(args, key)?
        .as_f64()
        .ok_or_else(|| format!("“{key}” must be a number"))
}

fn string<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    object(args, key)?
        .as_str()
        .ok_or_else(|| format!("“{key}” must be a string"))
}

fn circuit(args: &Map<String, Value>) -> Result<CircuitSpec, String> {
    serde_json::from_value(object(args, "circuit")?.clone())
        .map_err(|e| format!("invalid circuit: {e}"))
}

/// Schematics built from specs use only built-in blocks.
struct BuiltinsOnly;

impl Resolver for BuiltinsOnly {
    fn build_component(
        &self,
        _: DocId,
        _: &mut Circuit,
        _: &mut Vec<DocId>,
    ) -> Result<Ports, String> {
        Err("circuits from specs can only use built-in blocks".into())
    }
}

fn round(v: f64) -> f64 {
    if v.abs() < 1e-12 {
        0.0
    } else {
        (v * 1e9).round() / 1e9
    }
}

fn field_json(wavelength_nm: f64, field: C64) -> Value {
    json!({
        "wavelength_nm": wavelength_nm,
        "power": round(field.norm_sqr()),
        "re": round(field.re),
        "im": round(field.im),
        "phase_rad": round(if field.norm() < 1e-12 { 0.0 } else { field.arg() }),
    })
}

// ---- list_blocks ------------------------------------------------------------

fn list_blocks(_: &Map<String, Value>) -> Result<Value, String> {
    let blocks: Vec<Value> = Builtin::palette()
        .iter()
        .map(|b| {
            let info = b.info();
            let (kind, params) = spec::describe(b);
            json!({
                "type": kind,
                "name": info.name,
                "description": info.description,
                "category": info.category.title(),
                "inputs": b.inputs(),
                "outputs": b.outputs(),
                "params": params,
            })
        })
        .collect();
    Ok(json!({
        "blocks": blocks,
        "units": {"wavelength": "nm", "length": "µm", "phase": "rad", "loss": "dB, dB/cm", "power": "mW: |E|², a laser of amplitude 1 emits 1 mW (0 dBm)"},
    }))
}

// ---- simulate_circuit -----------------------------------------------------------

fn simulate_circuit(args: &Map<String, Value>) -> Result<Value, String> {
    let spec = circuit(args)?;
    let built = spec::build(&spec)?;
    let sim = schematic_sim::simulate(&built.schematic, &BuiltinsOnly, &mut Vec::new());
    if let Some(e) = sim.error {
        return Err(e);
    }
    let port_name = |p: PortRef, output: bool| {
        let block = built
            .schematic
            .instance(p.instance)
            .map(|i| match &i.block {
                BlockRef::Builtin(b) => {
                    if output {
                        b.outputs()
                    } else {
                        b.inputs()
                    }
                }
                BlockRef::Component(_) => Vec::new(),
            });
        let name = block
            .and_then(|names| names.get(p.port).cloned())
            .unwrap_or_else(|| p.port.to_string());
        format!("{}.{name}", built.name_of(p.instance))
    };
    let mut readings = Map::new();
    for (id, power) in &sim.readings {
        readings.insert(built.name_of(*id).to_owned(), json!(round(*power)));
    }
    let mut ports = Map::new();
    let mut keys: Vec<_> = sim.port_power.keys().copied().collect();
    keys.sort();
    for (instance, port) in keys {
        ports.insert(
            port_name(PortRef { instance, port }, true),
            json!(round(sim.port_power[&(instance, port)])),
        );
    }
    let connections: Vec<Value> = built
        .schematic
        .wires
        .iter()
        .enumerate()
        .map(|(k, w)| {
            json!({
                "from": port_name(w.from, true),
                "to": port_name(w.to, false),
                "power": round(sim.wire_power(k)),
                "fields": sim.wire_fields[k].iter().zip(&sim.wavelengths).map(|(f, wl)| field_json(*wl, *f)).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({
        "wavelengths_nm": sim.wavelengths,
        "readings": readings,
        "output_ports": ports,
        "connections": connections,
        "circuit_nodes": sim.nodes,
        "note": if sim.wavelengths.is_empty() { "No laser in the circuit, so everything is dark. Add a laser block." } else { "" },
    }))
}

// ---- sweep_wavelength -----------------------------------------------------------

fn sweep_wavelength(args: &Map<String, Value>) -> Result<Value, String> {
    let spec = circuit(args)?;
    let built = spec::build(&spec)?;
    let (start, stop) = (number(args, "start_nm")?, number(args, "stop_nm")?);
    let points = args.get("points").and_then(Value::as_u64).unwrap_or(201) as usize;
    if !(2..=5001).contains(&points) {
        return Err("“points” must be between 2 and 5001".into());
    }
    if !(start > 0.0 && stop > start) {
        return Err("need 0 < start_nm < stop_nm".into());
    }

    let mut circuit = Circuit::new();
    let compiled = schematic_sim::build_schematic(
        &built.schematic,
        &BuiltinsOnly,
        &mut circuit,
        &mut Vec::new(),
    )?;
    let instance = |name: &str| {
        built
            .ids
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
            .ok_or_else(|| format!("no block with id “{name}”"))
    };
    let port_index = |reference: &str, output: bool| -> Result<(String, PortRef), String> {
        let (block, port) = reference
            .rsplit_once(['.', ':'])
            .ok_or_else(|| format!("port “{reference}” should look like “block.port”"))?;
        let id = instance(block)?;
        let names = match &built.schematic.instance(id).expect("exists").block {
            BlockRef::Builtin(b) if output => b.outputs(),
            BlockRef::Builtin(b) => b.inputs(),
            BlockRef::Component(_) => Vec::new(),
        };
        let index = port
            .parse::<usize>()
            .ok()
            .filter(|&i| i < names.len())
            .or_else(|| names.iter().position(|n| n == port))
            .ok_or_else(|| {
                format!(
                    "“{block}” has no {} port “{port}”",
                    if output { "output" } else { "input" }
                )
            })?;
        Ok((
            format!("{block}.{}", names[index]),
            PortRef {
                instance: id,
                port: index,
            },
        ))
    };

    let input_ref = string(args, "input")?;
    let (input_name, input) = port_index(input_ref, false)?;
    if built.schematic.wire_into(input).is_some() {
        return Err(format!(
            "input “{input_name}” is already connected; sweep from an open input"
        ));
    }
    let drive: InPort = compiled.instance_ports[&input.instance].inputs[input.port];

    // Outputs: requested ports, or every detector and every open output.
    let mut outputs: Vec<(String, OutPort)> = Vec::new();
    match args.get("outputs").and_then(Value::as_array) {
        Some(list) => {
            for item in list {
                let r = item
                    .as_str()
                    .ok_or("“outputs” must be a list of port names")?;
                let (name, port) = port_index(r, true)?;
                outputs.push((
                    name,
                    compiled.instance_ports[&port.instance].outputs[port.port],
                ));
            }
        }
        None => {
            for (k, w) in built.schematic.wires.iter().enumerate() {
                if let Some(inst) = built.schematic.instance(w.to.instance)
                    && matches!(inst.block, BlockRef::Builtin(Builtin::Detector))
                {
                    outputs.push((built.name_of(inst.id).to_owned(), compiled.wire_sources[k]));
                }
            }
            for inst in &built.schematic.instances {
                for (p, &out) in compiled.instance_ports[&inst.id].outputs.iter().enumerate() {
                    let port = PortRef {
                        instance: inst.id,
                        port: p,
                    };
                    if built.schematic.wire_from(port).is_none()
                        && !matches!(inst.block, BlockRef::Builtin(Builtin::Laser { .. }))
                    {
                        let (name, _) =
                            port_index(&format!("{}.{p}", built.name_of(inst.id)), true)?;
                        outputs.push((name, out));
                    }
                }
            }
        }
    }
    if outputs.is_empty() {
        return Err("nothing to measure: add a detector or leave an output open".into());
    }

    let wavelengths: Vec<f64> = (0..points)
        .map(|k| start + (stop - start) * k as f64 / (points - 1) as f64)
        .collect();
    let ports: Vec<OutPort> = outputs.iter().map(|o| o.1).collect();
    let rows = crate::photonics::par::try_map(&wavelengths, |&wl| {
        circuit
            .transfer_matrix(wl, &[drive], &ports)
            .map(|t| {
                (0..ports.len())
                    .map(|i| t[(i, 0)].norm_sqr())
                    .collect::<Vec<f64>>()
            })
            .map_err(|e| e.to_string())
    })?;
    let mut series = Map::new();
    for (i, (name, _)) in outputs.iter().enumerate() {
        series.insert(
            name.clone(),
            json!(rows.iter().map(|r| round(r[i])).collect::<Vec<_>>()),
        );
    }
    Ok(json!({
        "input": input_name,
        "wavelengths_nm": wavelengths,
        "power": series,
        "note": "Power per unit input power; lasers in the circuit are ignored during a sweep.",
    }))
}

// ---- decompose_matrix ------------------------------------------------------------

fn parse_matrix(value: &Value) -> Result<Matrix, String> {
    let rows = value.as_array().ok_or("“matrix” must be a list of rows")?;
    if rows.is_empty() {
        return Err("“matrix” is empty".into());
    }
    let parse = |v: &Value| -> Result<C64, String> {
        match v {
            Value::Number(n) => Ok(C64::new(n.as_f64().unwrap_or(0.0), 0.0)),
            Value::Array(pair) if pair.len() == 2 => Ok(C64::new(
                pair[0]
                    .as_f64()
                    .ok_or("complex entries are [re, im] numbers")?,
                pair[1]
                    .as_f64()
                    .ok_or("complex entries are [re, im] numbers")?,
            )),
            _ => Err("entries must be numbers or [re, im] pairs".into()),
        }
    };
    let cols = rows[0].as_array().map_or(0, Vec::len);
    let mut data = Vec::new();
    for row in rows {
        let row = row.as_array().ok_or("each row must be a list")?;
        if row.len() != cols || cols == 0 {
            return Err("all rows must have the same, non-zero length".into());
        }
        for v in row {
            data.push(parse(v)?);
        }
    }
    if rows.len() * cols > 64 * 64 {
        return Err("matrix too large (max 64×64)".into());
    }
    Ok(Matrix::from_fn(rows.len(), cols, |i, j| data[i * cols + j]))
}

fn mesh_json(mesh: &UnitaryMesh) -> Value {
    json!({
        "size": mesh.size,
        "mzis": mesh.mzis.iter().map(|m| json!({"modes": [m.mode, m.mode + 1], "theta_rad": round(m.theta), "phi_rad": round(m.phi)})).collect::<Vec<_>>(),
        "output_phases_rad": mesh.output_phases.iter().map(|p| round(*p)).collect::<Vec<_>>(),
    })
}

fn max_error(a: &Matrix, b: &Matrix) -> f64 {
    (0..a.nrows())
        .flat_map(|i| (0..a.ncols()).map(move |j| (i, j)))
        .map(|(i, j)| (a[(i, j)] - b[(i, j)]).norm())
        .fold(0.0, f64::max)
}

fn decompose_matrix(args: &Map<String, Value>) -> Result<Value, String> {
    let m = parse_matrix(object(args, "matrix")?)?;
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("svd");
    match kind {
        "unitary" => {
            let mesh = UnitaryMesh::decompose(&m).map_err(|e| e.to_string())?;
            Ok(json!({
                "kind": "unitary",
                "mesh": mesh_json(&mesh),
                "mzi_count": mesh.mzis.len(),
                "max_reconstruction_error": max_error(&mesh.matrix(), &m),
                "convention": "MZI(θ, φ) = i·e^{iθ/2}[[e^{iφ}sin(θ/2), cos(θ/2)], [e^{iφ}cos(θ/2), −sin(θ/2)]]; light passes the MZIs in order, then the output phases.",
            }))
        }
        "svd" => {
            let mesh = SvdMesh::decompose(&m).map_err(|e| e.to_string())?;
            Ok(json!({
                "kind": "svd",
                "v_adjoint": mesh_json(&mesh.v_adjoint),
                "attenuation": mesh.attenuation.iter().map(|a| round(*a)).collect::<Vec<_>>(),
                "u": mesh_json(&mesh.u),
                "scale": mesh.scale,
                "mzi_count": mesh.v_adjoint.mzis.len() + mesh.u.mzis.len(),
                "max_reconstruction_error": max_error(&mesh.matrix(), &m),
                "convention": "M = scale · U · diag(attenuation) · V†. Optical output = M·x / scale.",
            }))
        }
        other => Err(format!("unknown kind “{other}”; use “svd” or “unitary”")),
    }
}

// ---- projects ------------------------------------------------------------------

pub fn read_project(path: &Path) -> Result<Project, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    if !Project::is_project(&bytes) {
        return Err(format!("{} is not a Shine project file", path.display()));
    }
    Project::from_bytes(&bytes)
}

fn simulate_project(args: &Map<String, Value>) -> Result<Value, String> {
    let path = Path::new(string(args, "path")?);
    let project = read_project(path)?;
    let catalog = project.catalog();
    let documents: Vec<Value> = project
        .documents
        .iter()
        .map(|doc| {
            let interface = doc.interface();
            let mut out = json!({
                "name": doc.name,
                "kind": if doc.component.is_some() { "component" } else { "layout" },
                "component": doc.component.as_ref().map(|c| json!({
                    "initials": c.initials,
                    "description": c.description,
                    "inputs": interface.inputs,
                    "outputs": interface.outputs,
                    "signal_inputs": interface.signal_inputs,
                    "signal_outputs": interface.signal_outputs,
                })),
            });
            let sim = project.simulate(doc.id, &Machine::default());
            if let Some(e) = sim.error {
                out["error"] = json!(e);
                return out;
            }
            let layout = doc.layout();
            let mut readings: Vec<Value> = sim
                .detector_power
                .iter()
                .map(|(i, power)| {
                    let c = &layout.components[*i];
                    let (x, y) = cell_of(c.pos);
                    let name = layout.label(*i).map_or_else(|| format!("{} at cell ({x}, {y})", c.kind.name()), str::to_owned);
                    json!({"block": name, "power": round(*power)})
                })
                .collect();
            // Detector and output-pin boxes read the waveguide feeding them.
            for (w, wire) in layout.wires.iter().enumerate() {
                let Some(inst) = layout.instance(wire.to.instance) else { continue };
                if matches!(inst.block, BlockRef::Builtin(Builtin::Detector | Builtin::OutputPin { .. })) {
                    let name = catalog.display(&inst.block).name;
                    readings.push(json!({"block": format!("{name} #{}", inst.id.0 + 1), "power": round(sim.wire_power(w))}));
                }
            }
            let displays: Vec<Value> = layout
                .instances
                .iter()
                .filter_map(|inst| match inst.block {
                    BlockRef::Builtin(Builtin::Display { bits }) => {
                        let values: Vec<f64> =
                            (0..bits as usize).map(|k| sim.input(layout, Terminal { item: Item::Box(inst.id), port: k })).collect();
                        Some(json!({"cell": [inst.cell.0, inst.cell.1], "value": bits_to_number(&values)}))
                    }
                    _ => None,
                })
                .collect();
            let signal_outputs: Vec<Value> = layout
                .components
                .iter()
                .enumerate()
                .filter(|(_, c)| c.kind == OpticComponentKind::SignalOut)
                .map(|(i, _)| {
                    let value = sim.input(layout, Terminal { item: Item::Part(i), port: 0 });
                    json!({"name": layout.label(i).unwrap_or("out"), "value": round(value)})
                })
                .collect();
            out["wavelengths_nm"] = json!(sim.wavelengths);
            out["readings"] = json!(readings);
            if !displays.is_empty() {
                out["displays"] = json!(displays);
            }
            if !signal_outputs.is_empty() {
                out["signal_outputs"] = json!(signal_outputs);
            }
            out
        })
        .collect();
    Ok(json!({"documents": documents}))
}

fn save_project(args: &Map<String, Value>) -> Result<Value, String> {
    let spec = circuit(args)?;
    let built = spec::build(&spec)?;
    let path = Path::new(string(args, "path")?);
    let overwrite = args
        .get("overwrite")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if path.exists() && !overwrite {
        return Err(format!(
            "{} already exists; pass \"overwrite\": true to replace it",
            path.display()
        ));
    }
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Circuit from AI");
    let mut project = Project::new();
    project.add_layout(name, v1::schematic_to_layout(built.schematic), 10.0);
    std::fs::write(path, project.to_bytes())
        .map_err(|e| format!("can't write {}: {e}", path.display()))?;
    Ok(
        json!({"path": path.display().to_string(), "document": name, "note": "Open this file in Shine (File → Open) to see and edit the circuit."}),
    )
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_blocks",
            title: "List building blocks",
            description: "Lists the photonic building blocks available for circuits: type name, ports, parameters with defaults, and units.",
            input_schema: || json!({"type": "object", "properties": {}}),
            call: list_blocks,
        },
        Tool {
            name: "simulate_circuit",
            title: "Simulate a photonic circuit",
            description: "Solves the steady state of a photonic circuit exactly (interference, loops and resonances included) at every laser wavelength. Returns detector readings, output-port powers, and the complex field on each connection.",
            input_schema: || json!({"type": "object", "properties": {"circuit": circuit_schema()}, "required": ["circuit"]}),
            call: simulate_circuit,
        },
        Tool {
            name: "sweep_wavelength",
            title: "Sweep a circuit's spectrum",
            description: "Injects unit power into one open input and returns the power at the outputs over a wavelength range. Use it for filters, rings and interferometers.",
            input_schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "circuit": circuit_schema(),
                        "input": {"type": "string", "description": "Open input port to drive, e.g. \"ring.in\""},
                        "start_nm": {"type": "number"},
                        "stop_nm": {"type": "number"},
                        "points": {"type": "integer", "minimum": 2, "maximum": 5001, "default": 201},
                        "outputs": {"type": "array", "items": {"type": "string"}, "description": "Output ports to measure; default: every detector and open output"}
                    },
                    "required": ["circuit", "input", "start_nm", "stop_nm"]
                })
            },
            call: sweep_wavelength,
        },
        Tool {
            name: "decompose_matrix",
            title: "Program an MZI mesh",
            description: "Finds MZI phase settings that implement a matrix on a photonic chip: \"unitary\" gives a triangular (Reck) mesh, \"svd\" handles any matrix with two meshes and attenuators.",
            input_schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "matrix": {"type": "array", "items": {"type": "array"}, "description": "Rows of numbers or [re, im] pairs, up to 64×64"},
                        "kind": {"type": "string", "enum": ["svd", "unitary"], "default": "svd"}
                    },
                    "required": ["matrix"]
                })
            },
            call: decompose_matrix,
        },
        Tool {
            name: "simulate_project",
            title: "Simulate a Shine project file",
            description: "Opens a project saved by the Shine editor and simulates every document, including reusable components, returning detector readings.",
            input_schema: || json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
            call: simulate_project,
        },
        Tool {
            name: "save_project",
            title: "Save a circuit for the editor",
            description: "Writes a circuit as a Shine project file, laid out left to right, so a person can open, inspect and edit it in the Shine editor.",
            input_schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "circuit": circuit_schema(),
                        "path": {"type": "string", "description": "Where to write the project, e.g. circuit.shine"},
                        "name": {"type": "string", "description": "Document name"},
                        "overwrite": {"type": "boolean", "default": false}
                    },
                    "required": ["circuit", "path"]
                })
            },
            call: save_project,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn mzi(phase: f64) -> Value {
        json!({
            "blocks": [
                {"id": "src", "type": "laser"},
                {"id": "dc1", "type": "coupler"},
                {"id": "ps", "type": "phase_shifter", "params": {"phase": phase}},
                {"id": "dc2", "type": "coupler"},
                {"id": "top", "type": "detector"},
                {"id": "bottom", "type": "detector"}
            ],
            "connections": [
                {"from": "src.out", "to": "dc1.in1"},
                {"from": "dc1.out1", "to": "ps.in"},
                {"from": "ps.out", "to": "dc2.in1"},
                {"from": "dc1.out2", "to": "dc2.in2"},
                {"from": "dc2.out1", "to": "top.in"},
                {"from": "dc2.out2", "to": "bottom.in"}
            ]
        })
    }

    #[test]
    fn list_blocks_describes_everything() {
        let out = list_blocks(&Map::new()).unwrap();
        let blocks = out["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), Builtin::palette().len());
        let coupler = blocks.iter().find(|b| b["type"] == "coupler").unwrap();
        assert_eq!(coupler["inputs"], json!(["in1", "in2"]));
        assert_eq!(coupler["params"]["coupling"], json!(0.5));
    }

    #[test]
    fn simulate_circuit_interferes() {
        let out = simulate_circuit(&args(json!({"circuit": mzi(0.0)}))).unwrap();
        assert_eq!(out["readings"]["top"], json!(0.0));
        assert_eq!(out["readings"]["bottom"], json!(1.0));
        assert_eq!(out["wavelengths_nm"], json!([1550.0]));
        assert_eq!(out["output_ports"]["dc1.out1"], json!(0.5));
        let first = &out["connections"][0];
        assert_eq!(
            (first["from"].as_str(), first["to"].as_str()),
            (Some("src.out"), Some("dc1.in1"))
        );
        assert_eq!(first["fields"][0]["power"], json!(1.0));

        let out = simulate_circuit(&args(json!({"circuit": mzi(std::f64::consts::PI)}))).unwrap();
        assert_eq!(out["readings"]["top"], json!(1.0));
    }

    #[test]
    fn simulate_circuit_reports_problems() {
        assert!(
            simulate_circuit(&Map::new())
                .unwrap_err()
                .contains("missing argument “circuit”")
        );
        assert!(
            simulate_circuit(&args(json!({"circuit": 5})))
                .unwrap_err()
                .contains("invalid circuit")
        );
        let dark = simulate_circuit(&args(
            json!({"circuit": {"blocks": [{"id": "pd", "type": "detector"}]}}),
        ))
        .unwrap();
        assert!(dark["note"].as_str().unwrap().contains("No laser"));
        // A lossless resonant loop has no steady state.
        let resonant = json!({"blocks": [
            {"id": "src", "type": "laser"}, {"id": "dc", "type": "coupler", "params": {"coupling": 0.0}},
            {"id": "pd", "type": "detector"}],
            "connections": [{"from": "src.out", "to": "dc.in1"}, {"from": "dc.out2", "to": "dc.in2"}, {"from": "dc.out1", "to": "pd.in"}]});
        assert!(
            simulate_circuit(&args(json!({"circuit": resonant})))
                .unwrap_err()
                .contains("steady state")
        );
    }

    #[test]
    fn sweep_shows_ring_resonance() {
        let ring = json!({"blocks": [{"id": "ring", "type": "add_drop_ring"}], "connections": []});
        let out = sweep_wavelength(&args(json!({"circuit": ring, "input": "ring.in", "start_nm": 1545.0, "stop_nm": 1555.0, "points": 201}))).unwrap();
        let drop = out["power"]["ring.drop"].as_array().unwrap();
        let through = out["power"]["ring.through"].as_array().unwrap();
        assert_eq!(drop.len(), 201);
        assert!(drop[100].as_f64().unwrap() > 0.99, "resonant at 1550 nm");
        assert!(through[0].as_f64().unwrap() > 0.9);

        let explicit = sweep_wavelength(&args(json!({"circuit": {"blocks": [{"id": "r", "type": "ring"}]}, "input": "r.0", "start_nm": 1540.0, "stop_nm": 1560.0, "points": 5, "outputs": ["r.through"]}))).unwrap();
        assert_eq!(explicit["power"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn sweep_measures_detectors_and_validates() {
        let out = sweep_wavelength(&args(json!({"circuit": mzi(0.0), "input": "dc1.in2", "start_nm": 1500.0, "stop_nm": 1600.0, "points": 3}))).unwrap();
        assert!(out["power"].get("top").is_some() && out["power"].get("bottom").is_some());
        let bad = |extra: Value| {
            let mut a = json!({"circuit": mzi(0.0), "input": "dc1.in2", "start_nm": 1500.0, "stop_nm": 1600.0});
            a.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            sweep_wavelength(&args(a)).unwrap_err()
        };
        assert!(bad(json!({"points": 1})).contains("between 2 and 5001"));
        assert!(bad(json!({"stop_nm": 1400.0})).contains("start_nm < stop_nm"));
        assert!(bad(json!({"input": "dc1.in1"})).contains("already connected"));
        assert!(bad(json!({"input": "dc1"})).contains("block.port"));
        assert!(bad(json!({"input": "zz.in"})).contains("no block"));
        assert!(bad(json!({"input": "dc1.nope"})).contains("no input port"));
        assert!(bad(json!({"outputs": [3]})).contains("list of port names"));
        assert!(bad(json!({"start_nm": "x"})).contains("must be a number"));
        let nothing = sweep_wavelength(&args(
            json!({"circuit": {"blocks": [{"id": "p", "type": "detector"}]}, "input": "p.in", "start_nm": 1.0, "stop_nm": 2.0}),
        ));
        assert!(nothing.unwrap_err().contains("nothing to measure"));
    }

    #[test]
    fn decompose_matrix_round_trips() {
        let out = decompose_matrix(&args(
            json!({"matrix": [[1, 2, 3, 4], [0, 1, 0, 2], [0, 0, 1, 3], [0, 0, 0, 1]]}),
        ))
        .unwrap();
        assert!(out["max_reconstruction_error"].as_f64().unwrap() < 1e-9);
        assert_eq!(out["mzi_count"], json!(12));
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let out = decompose_matrix(&args(
            json!({"kind": "unitary", "matrix": [[h, [0, h]], [[0, h], h]]}),
        ))
        .unwrap();
        assert_eq!(out["mzi_count"], json!(1));
        assert!(out["max_reconstruction_error"].as_f64().unwrap() < 1e-9);

        let err = |m: Value| decompose_matrix(&args(m)).unwrap_err();
        assert!(err(json!({"matrix": [[1, 2]], "kind": "unitary"})).contains("not square"));
        assert!(
            err(json!({"matrix": [[1, 1], [1, 1]], "kind": "unitary"})).contains("not unitary")
        );
        assert!(err(json!({"matrix": [[1]], "kind": "fft"})).contains("unknown kind"));
        assert!(err(json!({"matrix": []})).contains("empty"));
        assert!(err(json!({"matrix": [[1, 2], [3]]})).contains("same"));
        assert!(err(json!({"matrix": [["a"]]})).contains("numbers"));
        assert!(err(json!({"matrix": [[[1, "x"]]]})).contains("[re, im]"));
        assert!(err(json!({"matrix": 3})).contains("list of rows"));
        assert!(err(json!({"matrix": [3]})).contains("each row"));
        assert!(err(json!({"matrix": vec![vec![1; 65]; 65]})).contains("too large"));
    }

    #[test]
    fn projects_round_trip_through_files() {
        let dir = std::env::temp_dir().join(format!("shine-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ai.shine");
        let saved = save_project(&args(
            json!({"circuit": mzi(0.0), "path": path, "name": "MZI by AI"}),
        ))
        .unwrap();
        assert!(saved["note"].as_str().unwrap().contains("File → Open"));
        assert!(
            save_project(&args(json!({"circuit": mzi(0.0), "path": path})))
                .unwrap_err()
                .contains("already exists")
        );
        save_project(&args(
            json!({"circuit": mzi(0.0), "path": path, "name": "MZI by AI", "overwrite": true}),
        ))
        .unwrap();

        let out = simulate_project(&args(json!({"path": path}))).unwrap();
        let doc = &out["documents"][0];
        assert_eq!(doc["name"], "MZI by AI");
        let readings: Vec<f64> = doc["readings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["power"].as_f64().unwrap())
            .collect();
        assert_eq!(readings.len(), 2);
        assert!((readings.iter().sum::<f64>() - 1.0).abs() < 1e-9);

        // Projects made in the editor: components, layouts and electronics.
        let mut project = Project::new();
        crate::panels::optics::examples::import_component(&mut project, 0);
        project.add_layout("Fan-out", crate::panels::optics::examples::fan_out(), 10.0);
        crate::panels::optics::examples::eight_bit_adder(&mut project);
        let mut broken = crate::panels::optics::model::Layout::default();
        broken.add_instance(BlockRef::Component(DocId(999)), (0, 0));
        project.add_layout("Broken", broken, 10.0);
        let editor = dir.join("editor.shine");
        std::fs::write(&editor, project.to_bytes()).unwrap();
        let out = simulate_project(&args(json!({"path": editor}))).unwrap();
        let docs = out["documents"].as_array().unwrap();
        assert_eq!(docs.len(), 4);
        assert_eq!(docs[0]["component"]["initials"], "FA");
        assert_eq!(docs[0]["component"]["signal_inputs"], json!(["a", "b", "cin"]));
        assert_eq!(docs[1]["readings"].as_array().unwrap().len(), 4);
        assert_eq!(docs[2]["displays"][0]["value"], 155, "100 + 55");
        assert!(docs[3]["error"].as_str().unwrap().contains("deleted"));

        std::fs::write(
            dir.join("layout.json"),
            "{\"components\": [], \"lines\": []}",
        )
        .unwrap();
        assert!(
            simulate_project(&args(json!({"path": dir.join("layout.json")})))
                .unwrap_err()
                .contains("not a Shine project")
        );
        assert!(
            simulate_project(&args(json!({"path": dir.join("missing.shine")})))
                .unwrap_err()
                .contains("can't read")
        );
        assert!(
            save_project(&args(
                json!({"circuit": mzi(0.0), "path": dir.join("no/dir/x.shine")})
            ))
            .unwrap_err()
            .contains("can't write")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn every_tool_has_an_object_schema() {
        let tools = tools();
        assert_eq!(tools.len(), 6);
        for t in tools {
            assert_eq!((t.input_schema)()["type"], "object", "{}", t.name);
            assert!(!t.description.is_empty() && !t.title.is_empty());
        }
    }
}
