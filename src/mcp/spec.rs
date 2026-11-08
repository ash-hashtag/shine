//! A JSON circuit description for AI clients, converted to an editor schematic.
//!
//! ```json
//! {
//!   "blocks": [
//!     {"id": "src", "type": "laser", "params": {"wavelength_nm": 1550}},
//!     {"id": "dc", "type": "coupler"},
//!     {"id": "pd", "type": "detector"}
//!   ],
//!   "connections": [
//!     {"from": "src.out", "to": "dc.in1"},
//!     {"from": "dc.out2", "to": "pd.in", "length_um": 12.5}
//!   ]
//! }
//! ```
//!
//! Block types and parameters are those of [`Builtin`], in snake_case; ports
//! are named as listed by the `list_blocks` tool, or given as 0-based indices.

use std::collections::{HashMap, VecDeque};

use egui::Pos2;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::panels::schematic::model::{
    BlockRef, Builtin, InstanceId, PortRef, Schematic, WireModel,
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CircuitSpec {
    pub blocks: Vec<BlockSpec>,
    #[serde(default)]
    pub connections: Vec<ConnectionSpec>,
    /// Waveguide length per world unit for unconnected lengths; omit for ideal waveguides.
    #[serde(default)]
    pub um_per_unit: Option<f64>,
    #[serde(default)]
    pub n_eff: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BlockSpec {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub params: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectionSpec {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub length_um: Option<f64>,
}

/// `AddDropRing` -> `add_drop_ring`.
pub fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Type name and default parameters of a built-in block.
pub fn describe(block: &Builtin) -> (String, Map<String, Value>) {
    match serde_json::to_value(block).expect("blocks serialise") {
        Value::String(variant) => (snake_case(&variant), Map::new()),
        Value::Object(map) => {
            let (variant, params) = map.into_iter().next().expect("one variant");
            let params = match params {
                Value::Object(p) => p,
                _ => Map::new(),
            };
            (snake_case(&variant), params)
        }
        other => unreachable!("unexpected block encoding {other}"),
    }
}

/// Build a block from its type name and (partial) parameters.
pub fn builtin_from(kind: &str, params: &Map<String, Value>) -> Result<Builtin, String> {
    let wanted = kind.trim().to_lowercase().replace(['-', ' '], "_");
    let palette = Builtin::palette();
    let Some(default) = palette.iter().find(|b| describe(b).0 == wanted) else {
        let known: Vec<String> = palette.iter().map(|b| describe(b).0).collect();
        return Err(format!(
            "unknown block type “{kind}”; known types: {}",
            known.join(", ")
        ));
    };
    let (_, mut merged) = describe(default);
    for (key, value) in params {
        if !merged.contains_key(key) {
            let valid: Vec<&String> = merged.keys().collect();
            return Err(format!(
                "block type “{wanted}” has no parameter “{key}”; parameters: {}",
                if valid.is_empty() {
                    "none".to_owned()
                } else {
                    valid
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ));
        }
        merged.insert(key.clone(), value.clone());
    }
    let variant = match serde_json::to_value(default).expect("serialises") {
        Value::String(v) => {
            return serde_json::from_value(Value::String(v)).map_err(|e| e.to_string());
        }
        Value::Object(m) => m.keys().next().cloned().expect("one variant"),
        _ => unreachable!(),
    };
    let mut wrapper = Map::new();
    wrapper.insert(variant, Value::Object(merged));
    serde_json::from_value(Value::Object(wrapper))
        .map_err(|e| format!("bad parameters for “{wanted}”: {e}"))
}

/// A schematic built from a spec, with the mapping back to spec ids.
pub struct BuiltSpec {
    pub schematic: Schematic,
    pub ids: Vec<(String, InstanceId)>,
}

impl BuiltSpec {
    pub fn name_of(&self, id: InstanceId) -> &str {
        self.ids
            .iter()
            .find(|(_, i)| *i == id)
            .map_or("?", |(n, _)| n.as_str())
    }
}

fn resolve_port(
    reference: &str,
    blocks: &HashMap<&str, (InstanceId, &Builtin)>,
    output: bool,
) -> Result<PortRef, String> {
    let (block, port) = reference
        .rsplit_once(['.', ':'])
        .ok_or_else(|| format!("port “{reference}” should look like “block.port”"))?;
    let (instance, builtin) = blocks
        .get(block)
        .ok_or_else(|| format!("no block with id “{block}”"))?;
    let names = if output {
        builtin.outputs()
    } else {
        builtin.inputs()
    };
    let index = match port.parse::<usize>() {
        Ok(i) if i < names.len() => i,
        Ok(i) => {
            return Err(format!(
                "“{block}” has {} {} ports; {i} is out of range",
                names.len(),
                if output { "output" } else { "input" }
            ));
        }
        Err(_) => names.iter().position(|n| n == port).ok_or_else(|| {
            format!(
                "“{block}” has no {} port “{port}”; {} ports: {}",
                if output { "output" } else { "input" },
                if output { "output" } else { "input" },
                if names.is_empty() {
                    "none".to_owned()
                } else {
                    names.join(", ")
                }
            )
        })?,
    };
    Ok(PortRef {
        instance: *instance,
        port: index,
    })
}

/// Column of each block: its distance from the sources along connections.
fn layer_columns(spec: &CircuitSpec) -> HashMap<String, usize> {
    let mut incoming: HashMap<&str, usize> =
        spec.blocks.iter().map(|b| (b.id.as_str(), 0)).collect();
    let block_of = |r: &str| {
        r.rsplit_once(['.', ':'])
            .map(|(b, _)| b.to_owned())
            .unwrap_or_default()
    };
    let edges: Vec<(String, String)> = spec
        .connections
        .iter()
        .map(|c| (block_of(&c.from), block_of(&c.to)))
        .collect();
    for (_, to) in &edges {
        if let Some(n) = incoming.get_mut(to.as_str()) {
            *n += 1;
        }
    }
    let mut column: HashMap<String, usize> = HashMap::new();
    let mut queue: VecDeque<String> = spec
        .blocks
        .iter()
        .filter(|b| incoming.get(b.id.as_str()) == Some(&0))
        .map(|b| b.id.clone())
        .collect();
    for id in &queue {
        column.insert(id.clone(), 0);
    }
    // Longest-path layering, bounded so feedback loops terminate.
    let mut steps = 0;
    while let Some(id) = queue.pop_front() {
        steps += 1;
        if steps > spec.blocks.len() * spec.blocks.len().max(1) + 1 {
            break;
        }
        let c = column[&id];
        for (from, to) in &edges {
            if *from == id && column.get(to).is_none_or(|&t| t < c + 1) && c + 1 < spec.blocks.len()
            {
                column.insert(to.clone(), c + 1);
                queue.push_back(to.clone());
            }
        }
    }
    for b in &spec.blocks {
        column.entry(b.id.clone()).or_insert(0);
    }
    column
}

pub fn build(spec: &CircuitSpec) -> Result<BuiltSpec, String> {
    if spec.blocks.is_empty() {
        return Err("the circuit has no blocks".into());
    }
    let columns = layer_columns(spec);
    let mut rows: HashMap<usize, usize> = HashMap::new();
    let mut schematic = Schematic::default();
    let mut builtins = Vec::new();
    let mut ids = Vec::new();
    for b in &spec.blocks {
        if b.id.is_empty() || b.id.contains(['.', ':']) {
            return Err(format!(
                "block id “{}” must be non-empty and contain no “.” or “:”",
                b.id
            ));
        }
        if ids.iter().any(|(n, _)| n == &b.id) {
            return Err(format!("duplicate block id “{}”", b.id));
        }
        let builtin =
            builtin_from(&b.kind, &b.params).map_err(|e| format!("block “{}”: {e}", b.id))?;
        let column = columns[&b.id];
        let row = rows.entry(column).or_insert(0);
        let pos = Pos2::new(column as f32 * 180.0, *row as f32 * 120.0);
        *row += 1;
        let id = schematic.add(BlockRef::Builtin(builtin.clone()), pos);
        builtins.push(builtin);
        ids.push((b.id.clone(), id));
    }
    let lookup: HashMap<&str, (InstanceId, &Builtin)> = ids
        .iter()
        .zip(&builtins)
        .map(|((name, id), b)| (name.as_str(), (*id, b)))
        .collect();
    for c in &spec.connections {
        let from = resolve_port(&c.from, &lookup, true)?;
        let to = resolve_port(&c.to, &lookup, false)?;
        if schematic.wire_from(from).is_some() {
            return Err(format!(
                "output “{}” is connected twice; use a splitter to fan out",
                c.from
            ));
        }
        if schematic.wire_into(to).is_some() {
            return Err(format!(
                "input “{}” is connected twice; use a combiner to merge",
                c.to
            ));
        }
        schematic.connect(from, to);
        schematic
            .wires
            .last_mut()
            .expect("just connected")
            .length_um = c.length_um;
    }
    if let Some(um_per_unit) = spec.um_per_unit {
        schematic.wire_settings.model = WireModel::Geometric { um_per_unit };
    }
    if let Some(n_eff) = spec.n_eff {
        schematic.wire_settings.n_eff = n_eff;
    }
    Ok(BuiltSpec { schematic, ids })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(value: Value) -> CircuitSpec {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn names() {
        assert_eq!(snake_case("AddDropRing"), "add_drop_ring");
        assert_eq!(snake_case("Mzi"), "mzi");
        assert_eq!(describe(&Builtin::Detector).0, "detector");
        let (name, params) = describe(&Builtin::Coupler { coupling: 0.3 });
        assert_eq!(
            (name.as_str(), params["coupling"].as_f64()),
            ("coupler", Some(0.3))
        );
    }

    #[test]
    fn every_builtin_round_trips_through_its_type_name() {
        for b in Builtin::palette() {
            let (kind, params) = describe(&b);
            assert_eq!(builtin_from(&kind, &params).unwrap(), b, "{kind}");
        }
    }

    #[test]
    fn partial_params_and_errors() {
        let b = builtin_from("Phase-Shifter", json!({"phase": 1.5}).as_object().unwrap()).unwrap();
        assert_eq!(b, Builtin::PhaseShifter { phase: 1.5 });
        let laser =
            builtin_from("laser", json!({"wavelength_nm": 1310}).as_object().unwrap()).unwrap();
        assert!(
            matches!(laser, Builtin::Laser { wavelength_nm, amplitude, .. } if wavelength_nm == 1310.0 && amplitude == 1.0)
        );
        let e = builtin_from("flux_capacitor", &Map::new()).unwrap_err();
        assert!(
            e.contains("unknown block type") && e.contains("coupler"),
            "{e}"
        );
        let e = builtin_from("coupler", json!({"gain": 2}).as_object().unwrap()).unwrap_err();
        assert!(
            e.contains("no parameter “gain”") && e.contains("coupling"),
            "{e}"
        );
        let e = builtin_from("detector", json!({"x": 1}).as_object().unwrap()).unwrap_err();
        assert!(e.contains("parameters: none"), "{e}");
        let e =
            builtin_from("coupler", json!({"coupling": "lots"}).as_object().unwrap()).unwrap_err();
        assert!(e.contains("bad parameters"), "{e}");
    }

    #[test]
    fn builds_and_lays_out_left_to_right() {
        let built = build(&spec(json!({
            "blocks": [
                {"id": "pd", "type": "detector"},
                {"id": "src", "type": "laser"},
                {"id": "dc", "type": "coupler", "params": {"coupling": 0.2}}
            ],
            "connections": [
                {"from": "src.out", "to": "dc.in1"},
                {"from": "dc.1", "to": "pd.in", "length_um": 3.0}
            ],
            "n_eff": 3.1
        })))
        .unwrap();
        let s = &built.schematic;
        assert_eq!(s.wires.len(), 2);
        assert_eq!(s.wires[1].from.port, 1);
        assert_eq!(s.wires[1].length_um, Some(3.0));
        assert_eq!(s.wire_settings.n_eff, 3.1);
        let x = |name: &str| {
            s.instance(built.ids.iter().find(|i| i.0 == name).unwrap().1)
                .unwrap()
                .pos
                .x
        };
        assert!(x("src") < x("dc") && x("dc") < x("pd"));
        assert_eq!(built.name_of(s.instances[0].id), "pd");
        assert_eq!(built.name_of(InstanceId(99)), "?");
    }

    #[test]
    fn rejects_bad_specs() {
        let err = |v: Value| build(&spec(v)).err().unwrap();
        assert!(err(json!({"blocks": []})).contains("no blocks"));
        assert!(err(json!({"blocks": [{"id": "a.b", "type": "detector"}]})).contains("no “.”"));
        assert!(
            err(json!({"blocks": [{"id": "a", "type": "detector"}, {"id": "a", "type": "laser"}]}))
                .contains("duplicate")
        );
        assert!(err(json!({"blocks": [{"id": "a", "type": "nope"}]})).starts_with("block “a”"));
        let two = json!([{"id": "l", "type": "laser"}, {"id": "d", "type": "detector"}, {"id": "e", "type": "detector"}]);
        let with = |connections: Value| json!({"blocks": two, "connections": connections});
        assert!(err(with(json!([{"from": "l", "to": "d.in"}]))).contains("block.port"));
        assert!(
            err(with(json!([{"from": "x.out", "to": "d.in"}]))).contains("no block with id “x”")
        );
        assert!(err(with(json!([{"from": "l.in", "to": "d.in"}]))).contains("output ports: out"));
        assert!(err(with(json!([{"from": "l.out", "to": "d.7"}]))).contains("out of range"));
        assert!(err(with(json!([{"from": "d.out", "to": "l.in"}]))).contains("output ports: none"));
        assert!(
            err(with(
                json!([{"from": "l.out", "to": "d.in"}, {"from": "l.out", "to": "e.in"}])
            ))
            .contains("splitter")
        );
    }

    #[test]
    fn feedback_loops_still_lay_out() {
        let built = build(&spec(json!({
            "blocks": [{"id": "dc", "type": "coupler"}, {"id": "wg", "type": "delay_line"}],
            "connections": [{"from": "dc.out2", "to": "wg.in"}, {"from": "wg.out", "to": "dc.in2"}],
            "um_per_unit": 0.5
        })))
        .unwrap();
        assert_eq!(built.schematic.wires.len(), 2);
        assert!(matches!(
            built.schematic.wire_settings.model,
            WireModel::Geometric { .. }
        ));
    }
}
