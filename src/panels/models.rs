//! Components defined by data or a program rather than parts: S-parameters
//! measured or simulated elsewhere (Touchstone `.sNp`, Lumerical `.dat`),
//! used as black boxes, or a JavaScript program (see [`super::script`]). A
//! model sits in a [`Builtin::Model`](super::optics::model::Builtin) block;
//! importing a file makes a component around it, with a pin per port.

use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::photonics::{C64, Matrix, Table};

/// The speed of light, m/s.
const C: f64 = 299_792_458.0;

/// What a model block computes, and from what.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModelSpec {
    pub name: String,
    /// Light inputs (left edge, top to bottom) and outputs (right edge).
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    /// Signal inputs (top edge) and outputs (bottom edge): scripts only.
    pub signal_inputs: Vec<String>,
    pub signal_outputs: Vec<String>,
    pub source: ModelSource,
    /// The table built from `source`, on first use.
    #[serde(skip)]
    table: OnceLock<Arc<Table>>,
}

impl Clone for ModelSpec {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            signal_inputs: self.signal_inputs.clone(),
            signal_outputs: self.signal_outputs.clone(),
            source: self.source.clone(),
            table: OnceLock::new(),
        }
    }
}

impl PartialEq for ModelSpec {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.inputs == other.inputs
            && self.outputs == other.outputs
            && self.signal_inputs == other.signal_inputs
            && self.signal_outputs == other.signal_outputs
            && self.source == other.source
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ModelSource {
    /// The field from each input to each output at each wavelength
    /// (ascending): `values[k]` is the `outputs × inputs` matrix at
    /// `wavelengths_nm[k]`, row by row, as (re, im).
    Table { origin: String, wavelengths_nm: Vec<f64>, values: Vec<Vec<[f64; 2]>> },
    /// A JavaScript program, run in a sandbox ([`super::script`]).
    Script { code: String },
}

impl ModelSpec {
    pub fn new(name: impl Into<String>, inputs: Vec<String>, outputs: Vec<String>, source: ModelSource) -> Self {
        Self { name: name.into(), inputs, outputs, signal_inputs: Vec::new(), signal_outputs: Vec::new(), source, table: OnceLock::new() }
    }

    /// A scripted component's model, with the ports its script declares.
    pub fn from_script(code: &str) -> Result<Self, String> {
        let d = super::script::declared(code)?;
        Ok(Self {
            name: d.name,
            inputs: d.light_inputs,
            outputs: d.light_outputs,
            signal_inputs: d.signal_inputs,
            signal_outputs: d.signal_outputs,
            source: ModelSource::Script { code: code.to_owned() },
            table: OnceLock::new(),
        })
    }

    /// The script, for a scripted model.
    pub fn script(&self) -> Option<&str> {
        match &self.source {
            ModelSource::Script { code } => Some(code),
            ModelSource::Table { .. } => None,
        }
    }

    /// The model's scattering matrices, built once (empty for a script).
    pub fn table(&self) -> Arc<Table> {
        self.table
            .get_or_init(|| {
                let ModelSource::Table { wavelengths_nm, values, .. } = &self.source else {
                    return Arc::new(Table { wavelengths_nm: Vec::new(), matrices: Vec::new() });
                };
                let (o, i) = (self.outputs.len(), self.inputs.len());
                let matrices = values
                    .iter()
                    .map(|v| Matrix::from_fn(o, i, |r, c| v.get(r * i + c).map_or(C64::ZERO, |p| C64::new(p[0], p[1]))))
                    .collect();
                Arc::new(Table { wavelengths_nm: wavelengths_nm.clone(), matrices })
            })
            .clone()
    }

    /// "1530–1570 nm, 401 points, from ring.s4p"
    pub fn summary(&self) -> String {
        let ModelSource::Table { origin, wavelengths_nm, .. } = &self.source else {
            return "A component written in JavaScript".into();
        };
        match (wavelengths_nm.first(), wavelengths_nm.last()) {
            (Some(a), Some(b)) => format!("S-parameters from {origin}: {a:.1}–{b:.1} nm, {} points", wavelengths_nm.len()),
            _ => format!("S-parameters from {origin}"),
        }
    }

    /// A model from full `ports × ports` S-matrices at frequencies in Hz:
    /// which ports are inputs is given, or worked out (see [`split_ports`]).
    fn from_full(name: &str, origin: &str, frequencies_hz: &[f64], s: &[Matrix], inputs: Option<Vec<usize>>, port_names: &[String]) -> Result<Self, String> {
        let n = s.first().map_or(0, |m| m.nrows());
        if n < 2 {
            return Err("it needs at least two ports".into());
        }
        let inputs = inputs.unwrap_or_else(|| split_ports(s));
        let outputs: Vec<usize> = (0..n).filter(|p| !inputs.contains(p)).collect();
        if inputs.is_empty() || outputs.is_empty() {
            return Err("it needs ports on both sides".into());
        }
        let mut points: Vec<(f64, Vec<[f64; 2]>)> = frequencies_hz
            .iter()
            .zip(s)
            .filter(|(f, _)| **f > 0.0)
            .map(|(&f, m)| {
                let values = outputs.iter().flat_map(|&o| inputs.iter().map(move |&i| [m[(o, i)].re, m[(o, i)].im])).collect();
                (C / f * 1e9, values)
            })
            .collect();
        if points.is_empty() {
            return Err("it has no data".into());
        }
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        points.dedup_by(|a, b| a.0 == b.0);
        let name_of = |p: usize| port_names.get(p).cloned().unwrap_or_else(|| format!("{}", p + 1));
        let (wavelengths_nm, values) = points.into_iter().unzip();
        Ok(Self::new(
            name,
            inputs.iter().map(|&p| name_of(p)).collect(),
            outputs.iter().map(|&p| name_of(p)).collect(),
            ModelSource::Table { origin: origin.to_owned(), wavelengths_nm, values },
        ))
    }

    /// A Touchstone file (version 1 or 2), read with `rust-rf`. `name` is
    /// the file's name: `.sNp` says how many ports a version 1 file has.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_touchstone(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let rank = super::interop::touchstone::ports_of(name).unwrap_or(0);
        let ts = rust_rf::io::touchstone::Touchstone::from_reader(bytes, rank).map_err(|e| e.to_string())?;
        let (f, s) = ts.s_parameter_arrays();
        let (points, n, _) = s.dim();
        let matrices: Vec<Matrix> = (0..points).map(|k| Matrix::from_fn(n, n, |o, i| s[[k, o, i]])).collect();
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        Self::from_full(stem, name, f.as_slice().unwrap_or(&f.to_vec()), &matrices, None, &[])
    }

    #[cfg(target_arch = "wasm32")]
    pub fn from_touchstone(_name: &str, _bytes: &[u8]) -> Result<Self, String> {
        Err("S-parameter files can be read in the desktop app".into())
    }

    /// A Lumerical INTERCONNECT S-parameter file (`.dat`), as foundry kits
    /// such as SiEPIC EBeam publish their compact models: optional port
    /// lines `['port 1','LEFT']`, then for each pair of ports a header
    /// `('port 2',TE,1,'port 1',1,'transmission')` (to, mode, mode id, from,
    /// mode id; quoted with `'` or `"`, or not), its shape `(rows,3)` and
    /// rows of frequency (Hz), magnitude and phase (radians). Only the first
    /// mode is read.
    pub fn from_lumerical(name: &str, text: &str) -> Result<Self, String> {
        let mut sides: Vec<(String, String)> = Vec::new();
        // (to, from) port names → rows of (f, magnitude, phase).
        let mut blocks: Vec<(String, String, Vec<[f64; 3]>)> = Vec::new();
        let mut first_mode: Option<String> = None;
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty()).peekable();
        // The fields of `('a',b,1)` or `["a","b"]`, unquoted.
        let quoted = |l: &str| -> Vec<String> {
            l.trim_matches(|c| matches!(c, '(' | ')' | '[' | ']'))
                .split(',')
                .map(|f| f.trim().trim_matches(|c| c == '\'' || c == '"').to_owned())
                .collect()
        };
        // A header has letters in it; a block's shape is only numbers.
        let is_header = |l: &str| l.starts_with('(') && l.chars().any(|c| c.is_ascii_alphabetic());
        while let Some(line) = lines.next() {
            if line.starts_with('[') {
                let q = quoted(line);
                if q.len() >= 2 {
                    sides.push((q[0].clone(), q[1].to_uppercase()));
                }
            } else if is_header(line) {
                let q = quoted(line);
                if q.len() < 4 {
                    return Err(format!("can't read the header “{line}”"));
                }
                let mode = q[1].clone();
                let skip_block = first_mode.get_or_insert_with(|| mode.clone()) != &mode || !line.to_lowercase().contains("transmission");
                let shape = lines.next().ok_or("a block ends early")?;
                let rows: usize = shape
                    .trim_matches(|c| c == '(' || c == ')')
                    .split(',')
                    .next()
                    .and_then(|r| r.trim().parse().ok())
                    .ok_or_else(|| format!("can't read the block size “{shape}”"))?;
                let mut data = Vec::with_capacity(rows);
                for _ in 0..rows {
                    let row = lines.next().ok_or("a block has fewer rows than it says")?;
                    let v: Vec<f64> = row.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
                    if v.len() < 3 {
                        return Err(format!("can't read the row “{row}”"));
                    }
                    data.push([v[0], v[1], v[2]]);
                }
                if !skip_block {
                    blocks.push((q[0].clone(), q[3].clone(), data));
                }
            }
        }
        if blocks.is_empty() {
            return Err("no S-parameters found (is it an INTERCONNECT .dat file?)".into());
        }
        // Ports in the order they're declared, else first seen.
        let mut ports: Vec<String> = sides.iter().map(|s| s.0.clone()).collect();
        for (to, from, _) in &blocks {
            for p in [to, from] {
                if !ports.contains(p) {
                    ports.push(p.clone());
                }
            }
        }
        let n = ports.len();
        let frequencies: Vec<f64> = blocks[0].2.iter().map(|r| r[0]).collect();
        if blocks.iter().any(|b| b.2.len() != frequencies.len()) {
            return Err("its blocks have different frequencies".into());
        }
        let index = |p: &str| ports.iter().position(|x| x == p).expect("listed");
        let mut matrices: Vec<Matrix> = vec![Matrix::zeros(n, n); frequencies.len()];
        for (to, from, rows) in &blocks {
            let (o, i) = (index(to), index(from));
            for (k, r) in rows.iter().enumerate() {
                matrices[k][(o, i)] = C64::from_polar(r[1], r[2]);
            }
        }
        // Left ports take light in, right ports give it out.
        let side = |p: &str| sides.iter().find(|s| s.0 == p).map(|s| s.1.clone());
        let inputs = (sides.len() == n && sides.iter().any(|s| s.1 == "LEFT") && sides.iter().any(|s| s.1 == "RIGHT"))
            .then(|| (0..n).filter(|&p| side(&ports[p]).is_some_and(|s| s == "LEFT" || s == "TOP")).collect());
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        Self::from_full(stem, name, &frequencies, &matrices, inputs, &ports)
    }

    /// A model from a file it recognises by name: `.sNp`, `.ts` or `.dat`.
    pub fn from_file(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let lower = name.to_lowercase();
        if lower.ends_with(".dat") {
            let text = std::str::from_utf8(bytes).map_err(|_| format!("{name} isn't text"))?;
            Self::from_lumerical(name, text)
        } else if is_touchstone(name) {
            Self::from_touchstone(name, bytes)
        } else {
            Err(format!("{name} isn't an S-parameter file (.sNp or .dat)"))
        }
    }
}

/// A component around a model: a pin for each of its ports, wired to the
/// model block, as a one-component project (what the library keeps).
pub fn component(model: ModelSpec) -> crate::panels::project::Project {
    let name = model.name.clone();
    let description = model.summary();
    let layout = component_layout(model);
    let mut project = crate::panels::project::Project::new();
    project.add_component(&name, &crate::panels::ui_kit::initials(&name), &description, layout, 10.0);
    project
}

/// The inside of a model's component: the model block, a light pin for each
/// light port, and a signal pin above (in) or below (out) each signal port.
pub fn component_layout(model: ModelSpec) -> crate::panels::optics::model::Layout {
    use crate::panels::optics::model::{BlockRef, Builtin, Item, Layout, OpticComponentKind, PlacedComponent, PortRef, Terminal, cell_center};
    let rows = model.inputs.len().max(model.outputs.len()) as i32;
    let width = model.signal_inputs.len().max(model.signal_outputs.len()).max(2) as i32;
    let (inputs, outputs) = (model.inputs.clone(), model.outputs.clone());
    let (signal_inputs, signal_outputs) = (model.signal_inputs.clone(), model.signal_outputs.clone());
    let mut layout = Layout::default();
    let block = layout.add_instance(BlockRef::Builtin(Builtin::Model(Arc::new(model))), (4, 0));
    let pr = |instance, port| PortRef { instance, port };
    for (k, port) in inputs.iter().enumerate() {
        let pin = layout.add_instance(BlockRef::Builtin(Builtin::InputPin { name: port.clone() }), (0, 2 * k as i32 - rows + 1));
        layout.connect_optical(pr(pin, 0), pr(block, k));
    }
    for (k, port) in outputs.iter().enumerate() {
        let pin = layout.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: port.clone() }), (width + 7, 2 * k as i32 - rows + 1));
        layout.connect_optical(pr(block, k), pr(pin, 0));
    }
    for (k, name) in signal_inputs.iter().enumerate() {
        let i = layout.components.len();
        layout.components.push(PlacedComponent { kind: OpticComponentKind::SignalIn, pos: cell_center(4 + k as i32, -rows - 3 - k as i32 % 2) });
        layout.set_label(i, name);
        layout.connect_signal(Terminal { item: Item::Part(i), port: 0 }, Terminal { item: Item::Box(block), port: k });
    }
    for (k, name) in signal_outputs.iter().enumerate() {
        let i = layout.components.len();
        layout.components.push(PlacedComponent { kind: OpticComponentKind::SignalOut, pos: cell_center(4 + k as i32, rows + 3 + k as i32 % 2) });
        layout.set_label(i, name);
        layout.connect_signal(Terminal { item: Item::Box(block), port: k }, Terminal { item: Item::Part(i), port: 0 });
    }
    layout
}

/// Whether a file name is a Touchstone file.
pub fn is_touchstone(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".ts") || super::interop::touchstone::ports_of(&lower).is_some()
}

/// Whether a file name is one [`ModelSpec::from_file`] reads.
pub fn is_model_file(name: &str) -> bool {
    is_touchstone(name) || name.to_lowercase().ends_with(".dat")
}

/// Which ports take light in: the split that carries the most power from
/// inputs to outputs over all frequencies; on a tie, the lowest-numbered
/// ports as inputs. More than 12 ports: the first half.
pub fn split_ports(s: &[Matrix]) -> Vec<usize> {
    let n = s.first().map_or(0, |m| m.nrows());
    if n > 12 {
        return (0..n / 2).collect();
    }
    let power = |inputs: u32| -> f64 {
        s.iter()
            .map(|m| {
                let mut sum = 0.0;
                for i in (0..n).filter(|i| inputs & (1 << i) != 0) {
                    for o in (0..n).filter(|o| inputs & (1 << o) == 0) {
                        sum += m[(o, i)].norm_sqr();
                    }
                }
                sum
            })
            .sum()
    };
    let mut best: Option<(f64, u32)> = None;
    for mask in 1..(1u32 << n) - 1 {
        let p = power(mask);
        let better = match best {
            None => true,
            // Lower-numbered inputs win a tie: compare the masks bit by bit from port 1.
            Some((bp, bm)) => p > bp * (1.0 + 1e-9) || ((p - bp).abs() <= bp * 1e-9 && mask.reverse_bits() > bm.reverse_bits()),
        };
        if better {
            best = Some((p, mask));
        }
    }
    let mask = best.map_or(1, |b| b.1);
    (0..n).filter(|i| mask & (1 << i) != 0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::tests_support::splitter_s3p;

    #[test]
    fn a_touchstone_splitter_has_one_input_and_two_outputs() {
        let model = ModelSpec::from_file("y-branch.s3p", splitter_s3p().as_bytes()).unwrap();
        assert_eq!(model.name, "y-branch");
        assert_eq!((model.inputs.clone(), model.outputs.clone()), (vec!["1".to_owned()], vec!["2".to_owned(), "3".to_owned()]));
        let t = model.table();
        assert!(t.wavelengths_nm.windows(2).all(|w| w[0] < w[1]), "ascending wavelength");
        assert!((t.at(1530.0)[(1, 0)].norm_sqr() - 0.5).abs() < 1e-9);
        // Through serde (as in a project file) and back: equal, table rebuilt.
        let copy: ModelSpec = postcard::from_bytes(&postcard::to_allocvec(&model).unwrap()).unwrap();
        assert_eq!(copy, model);
        assert_eq!(copy.table().matrices, t.matrices);
    }

    #[test]
    fn a_couplers_ports_split_by_where_the_light_goes() {
        // Ports 1 and 3 are one side, 2 and 4 the other (an unusual numbering).
        let k = C64::new(0.5f64.sqrt(), 0.0);
        let m = Matrix::from_fn(4, 4, |r, c| if (r % 2) != (c % 2) { k } else { C64::ZERO });
        assert_eq!(split_ports(&[m]), vec![0, 2]);
    }

    #[test]
    fn reads_lumerical_dat_files() {
        let text = r#"["port 1","LEFT"]
["port 2","RIGHT"]
["port 3","RIGHT"]
("port 1","TE",1,"port 1",1,"transmission")
(2,3)
1.9e14 0.01 0
2.0e14 0.01 0
("port 2","TE",1,"port 1",1,"transmission")
(2,3)
1.9e14 0.7 0.5
2.0e14 0.6 1.0
("port 3","TE",1,"port 1",1,"transmission")
(2,3)
1.9e14 0.7 0.5
2.0e14 0.6 1.0
("port 2","TM",2,"port 1",2,"transmission")
(2,3)
1.9e14 0.1 0
2.0e14 0.1 0
"#;
        let model = ModelSpec::from_file("ebeam_y_1550.dat", text.as_bytes()).unwrap();
        assert_eq!(model.inputs, vec!["port 1"]);
        assert_eq!(model.outputs, vec!["port 2", "port 3"]);
        let t = model.table();
        // 2.0e14 Hz is the shorter wavelength: first.
        let first = t.matrices[0][(0, 0)];
        assert!((first.norm() - 0.6).abs() < 1e-12 && (first.arg() - 1.0).abs() < 1e-12, "the TE mode, not TM: {first}");
        assert!(ModelSpec::from_file("notes.dat", b"hello").is_err());
    }
}

#[cfg(test)]
mod component_tests {
    use super::*;
    use crate::panels::optics::model::{BlockRef, Builtin, Layout, PortRef};
    use crate::panels::project::Project;

    #[test]
    fn a_model_component_simulates_like_its_data_and_is_saved() {
        let s3p = super::tests_support::splitter_s3p();
        let model = ModelSpec::from_file("y-branch.s3p", s3p.as_bytes()).unwrap();
        let bundle = component(model);
        let doc = &bundle.documents[0];
        assert_eq!(doc.interface().inputs, vec!["1"]);
        assert_eq!(doc.interface().outputs, vec!["2", "3"]);
        assert!(doc.component.as_ref().unwrap().description.contains("S-parameters from y-branch.s3p"));

        // Used in a design: a 1 mW laser splits in two halves.
        let mut p = Project::new();
        let y = p.import(&bundle).unwrap();
        let mut l = Layout::default();
        let laser = l.add_instance(BlockRef::Builtin(Builtin::Laser { amplitude: 1.0, phase: 0.0, wavelength_nm: 1550.0 }), (0, 0));
        let b = l.add_instance(BlockRef::Component(y), (4, 0));
        l.connect_optical(PortRef { instance: laser, port: 0 }, PortRef { instance: b, port: 0 });
        let top = p.add_layout("Top", l, 10.0);
        let o = p.simulate(top, &Default::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        assert!((o.port_power[&(b, 0)] - 0.5).abs() < 1e-9 && (o.port_power[&(b, 1)] - 0.5).abs() < 1e-9, "{:?}", o.port_power);

        // Saved and opened again: the same data and results.
        let back = Project::from_bytes(&p.to_bytes()).unwrap();
        let o2 = back.simulate(top, &Default::default());
        assert_eq!(o2.port_power[&(b, 0)], o.port_power[&(b, 0)]);
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use crate::panels::interop::touchstone::SParameters;

    /// A 1×2 splitter's file: port 1 in, ports 2 and 3 out, reciprocal.
    pub fn splitter_s3p() -> String {
        let half = C64::new(std::f64::consts::FRAC_1_SQRT_2, 0.0);
        let m = Matrix::from_fn(3, 3, |r, c| if (r == 0) != (c == 0) { half } else { C64::ZERO });
        SParameters { ports: 3, frequencies_hz: vec![1.9e14, 1.95e14, 2.0e14], matrices: vec![m.clone(), m.clone(), m] }.to_touchstone(&[])
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod script_tests {
    use super::*;
    use crate::panels::circuit::sim::{self, Machine};
    use crate::panels::optics::model::{BlockRef, Builtin, Item, Layout, PortRef, Terminal};
    use crate::panels::project::Project;

    /// A design: a 1 mW laser into the template's splitter, its `split`
    /// input driven by a number input when `split` is given.
    fn design(split: Option<u64>, code: &str) -> (Project, crate::panels::project::DocId, crate::panels::optics::model::InstanceId) {
        let mut p = Project::new();
        let c = p.import(&component(ModelSpec::from_script(code).unwrap())).unwrap();
        let mut l = Layout::default();
        let laser = l.add_instance(BlockRef::Builtin(Builtin::Laser { amplitude: 1.0, phase: 0.0, wavelength_nm: 1550.0 }), (0, 0));
        let b = l.add_instance(BlockRef::Component(c), (4, 0));
        l.connect_optical(PortRef { instance: laser, port: 0 }, PortRef { instance: b, port: 0 });
        if let Some(v) = split {
            let n = l.add_instance(BlockRef::Builtin(Builtin::NumberInput { bits: 1, value: v }), (4, -4));
            l.connect_signal(Terminal { item: Item::Box(n), port: 0 }, Terminal { item: Item::Box(b), port: 0 });
        }
        let top = p.add_layout("Top", l, 10.0);
        (p, top, b)
    }

    #[test]
    fn a_scripted_component_computes_its_light_and_signals() {
        let (p, top, b) = design(None, crate::panels::script::TEMPLATE);
        let o = p.simulate(top, &Default::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        assert!((o.port_power[&(b, 0)] - 1.0).abs() < 1e-9 && o.port_power[&(b, 1)] < 1e-12, "not driven (reads 0): all to out1");

        // Driven by a signal: all of it to out2.
        let (p, top, b) = design(Some(1), crate::panels::script::TEMPLATE);
        let o = p.simulate(top, &Default::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        assert!(o.port_power[&(b, 0)] < 1e-12 && (o.port_power[&(b, 1)] - 1.0).abs() < 1e-9, "{:?}", o.port_power);

        // Its state moves on with the clock: three ticks, and it says 3.
        let (p, top, b) = design(None, crate::panels::script::TEMPLATE);
        let layout = p.doc(top).unwrap().layout().clone();
        let mut machine = Machine::default();
        for _ in 0..3 {
            let o = sim::simulate(&layout, 10.0, &p, &machine, Some(top), &Default::default());
            sim::clock(&layout, &p, &mut machine, &o);
        }
        let o = sim::simulate(&layout, 10.0, &p, &machine, Some(top), &Default::default());
        assert_eq!(o.signal(Item::Box(b), 0), 3.0);
    }

    #[test]
    fn a_failing_script_is_the_simulations_error() {
        let code = "const component = { name: 'Broken', light: { inputs: ['a'], outputs: ['b'] },\n  transmission() { throw new Error('no data here'); } };";
        let (p, top, _) = design(None, code);
        let o = p.simulate(top, &Default::default());
        let e = o.error.expect("an error");
        assert!(e.contains("Broken") && e.contains("no data here") && e.contains("line 2"), "{e}");
    }
}
