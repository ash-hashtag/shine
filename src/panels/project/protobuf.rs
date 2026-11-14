//! Projects in Shine's protobuf format (`crates/shine-format`): a project as a
//! bundle of documents, each under the path it has in a project folder, and
//! boxes referring to components by path.

use std::collections::HashMap;
use std::sync::Arc;

use emath::{Pos2, Vec2};
use shine_format as pb;
use shine_format::{mod_Builtin::OneOfkind as B, mod_Instance::OneOfblock, mod_Item::OneOfitem, mod_Part::OneOfkind as P};

use super::{ComponentMeta, DocId, DocumentFile};
use crate::panels::appearance::{Appearance, BoxShape, Icon, IconFormat};
use crate::panels::models::{ModelSource, ModelSpec};
use crate::panels::optics::model::{
    BlockRef, Builtin, Export, Feed, Instance, InstanceId, Item, Layout, Line, OpticComponentKind, PlacedComponent, PortRef, SignalWire, Terminal,
    Wire,
};

pub const DESIGN_EXTENSION: &str = "shinedesign";
pub const COMPONENT_EXTENSION: &str = "shinecomp";

/// A file name from a document's name: lower case, words joined by dashes.
pub fn slug(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if words.is_empty() { "untitled".into() } else { words.join("-") }
}

/// Each document's path in a project folder: designs at the top, components
/// in `components/`, named after them (numbered when two would clash).
pub(crate) fn paths(documents: &[DocumentFile]) -> Vec<String> {
    let mut taken = std::collections::HashSet::new();
    documents
        .iter()
        .map(|d| {
            let (folder, extension) = if d.component.is_some() { ("components/", COMPONENT_EXTENSION) } else { ("", DESIGN_EXTENSION) };
            let base = slug(&d.name);
            let mut n = 1;
            loop {
                let path = if n == 1 { format!("{folder}{base}.{extension}") } else { format!("{folder}{base}-{n}.{extension}") };
                if taken.insert(path.to_lowercase()) {
                    return path;
                }
                n += 1;
            }
        })
        .collect()
}

/// The folder part of a path ("" at the top).
fn folder_of(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

/// `to` written relative to the folder `from` ("/" separators).
pub fn relative(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let common = from.iter().zip(&to_parts).take_while(|(a, b)| a == b).count();
    let mut out: Vec<&str> = vec![".."; from.len() - common];
    out.extend(&to_parts[common..]);
    out.join("/")
}

/// `path`, relative to the folder `from`, as a path from the top; `None` if
/// it climbs above the top.
pub fn resolve(from: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

// ---- writing ------------------------------------------------------------------

/// A project's documents as a bundle.
pub(crate) fn bundle(documents: &[DocumentFile]) -> pb::Bundle {
    let paths = paths(documents);
    let path_of: HashMap<DocId, &str> = documents.iter().zip(&paths).map(|(d, p)| (d.id, p.as_str())).collect();
    let files = documents
        .iter()
        .zip(&paths)
        .map(|(d, path)| {
            let here = folder_of(path);
            let reference = |id: DocId| path_of.get(&id).map(|to| relative(here, to));
            pb::BundledFile { path: path.clone(), document: Some(document(d, &reference)), id: d.id.0 }
        })
        .collect();
    pb::Bundle { files }
}

fn document(d: &DocumentFile, reference: &dyn Fn(DocId) -> Option<String>) -> pb::Document {
    pb::Document {
        name: d.name.clone(),
        component: d.component.as_ref().map(component),
        cell_um: d.cell_um,
        layout: Some(layout(&d.layout, reference)),
    }
}

fn component(c: &ComponentMeta) -> pb::ComponentInfo {
    pb::ComponentInfo { initials: c.initials.clone(), description: c.description.clone(), appearance: Some(appearance(&c.appearance)) }
}

fn color(c: [u8; 3]) -> pb::Color {
    pb::Color { rgb: u32::from_be_bytes([0, c[0], c[1], c[2]]) }
}

fn appearance(a: &Appearance) -> pb::Appearance {
    pb::Appearance {
        color: a.color.map(color),
        shape: match a.shape {
            BoxShape::Rectangle => pb::BoxShape::RECTANGLE,
            BoxShape::Rounded => pb::BoxShape::ROUNDED,
            BoxShape::Capsule => pb::BoxShape::CAPSULE,
            BoxShape::Chamfered => pb::BoxShape::CHAMFERED,
        },
        icon: a.icon.as_ref().map(|i| pb::Icon {
            format: match i.format {
                IconFormat::WebP => pb::IconFormat::WEBP,
                IconFormat::Svg => pb::IconFormat::SVG,
            },
            data: i.bytes.to_vec(),
        }),
    }
}

fn point(p: Pos2) -> pb::Point {
    pb::Point { x: p.x, y: p.y }
}

fn vector(v: Vec2) -> pb::Point {
    pb::Point { x: v.x, y: v.y }
}

fn layout(l: &Layout, reference: &dyn Fn(DocId) -> Option<String>) -> pb::Layout {
    pb::Layout {
        parts: l.components.iter().map(part).collect(),
        lines: l.lines.iter().map(|l| pb::Line { start: Some(point(l.start)), end: Some(point(l.end)) }).collect(),
        boxes: l.instances.iter().map(|i| instance(i, reference)).collect(),
        waveguides: l
            .wires
            .iter()
            .map(|w| pb::Waveguide { from: Some(port(w.from)), to: Some(port(w.to)), length: w.length_um.map(|um| pb::Length { um }) })
            .collect(),
        signals: l.signals.iter().map(|s| pb::SignalWire { from: Some(terminal(s.from)), to: Some(terminal(s.to)) }).collect(),
        next_box: l.next_instance,
        labels: l.labels.iter().map(|(part, text)| pb::Label { part: *part as u32, text: text.clone() }).collect(),
        feeds: l.feeds.iter().map(|f| pb::Feed { laser: f.laser as u32, to: Some(port(f.to)) }).collect(),
        exports: l.exports.iter().map(|e| pb::Export { name: e.name.clone(), item: Some(item(e.item)), parameter: e.param.clone() }).collect(),
    }
}

fn part(c: &PlacedComponent) -> pb::Part {
    use OpticComponentKind as K;
    let kind = match c.kind {
        K::Laser { amplitude, phase, wavelength, dir, source, color: c } => P::laser(pb::FreeLaser {
            amplitude,
            phase,
            wavelength_nm: wavelength,
            direction: Some(vector(dir)),
            source: source as u32,
            color: c.map(color),
        }),
        K::BeamSplitter { reflectivity, transparency, normal } => {
            P::beam_splitter(pb::BeamSplitter { reflectivity, transparency, normal: Some(vector(normal)) })
        }
        K::Mirror { normal } => P::mirror(pb::Mirror { normal: Some(vector(normal)) }),
        K::Lens => P::lens(pb::Empty {}),
        K::PhaseShifter { phase } => P::phase_shifter(pb::FreePhaseShifter { phase }),
        K::Attenuator { transmission } => P::attenuator(pb::FreeAttenuator { transmission }),
        K::Detector { threshold } => P::detector(pb::FreeDetector { threshold }),
        K::InputPort { dir } => P::input_port(pb::InputPort { direction: Some(vector(dir)) }),
        K::OutputPort => P::output_port(pb::Empty {}),
        K::SignalIn => P::signal_in(pb::Empty {}),
        K::SignalOut => P::signal_out(pb::Empty {}),
    };
    pb::Part { position: Some(point(c.pos)), kind }
}

fn port(p: PortRef) -> pb::PortRef {
    pb::PortRef { box_pb: p.instance.0, port: p.port as u32 }
}

fn item(i: Item) -> pb::Item {
    pb::Item {
        item: match i {
            Item::Part(k) => OneOfitem::part(k as u32),
            Item::Box(id) => OneOfitem::box_pb(id.0),
        },
    }
}

fn terminal(t: Terminal) -> pb::Terminal {
    pb::Terminal { item: Some(item(t.item)), port: t.port as u32 }
}

fn instance(i: &Instance, reference: &dyn Fn(DocId) -> Option<String>) -> pb::Instance {
    pb::Instance {
        id: i.id.0,
        column: i.cell.0,
        row: i.cell.1,
        block: match &i.block {
            BlockRef::Builtin(b) => OneOfblock::builtin(builtin(b)),
            // A reference to a document the bundle doesn't have can't be
            // written as a path; it reads back as a missing component.
            BlockRef::Component(id) => OneOfblock::component(reference(*id).unwrap_or_default()),
        },
        parameters: i.params.iter().map(|(name, value)| pb::Parameter { name: name.clone(), value: *value }).collect(),
    }
}

fn builtin(b: &Builtin) -> pb::Builtin {
    let kind = match b {
        Builtin::Laser { amplitude, phase, wavelength_nm } => {
            B::laser(pb::Laser { amplitude: *amplitude, phase: *phase, wavelength_nm: *wavelength_nm })
        }
        Builtin::Detector => B::detector(pb::Empty {}),
        Builtin::InputPin { name } => B::input_pin(pb::Pin { name: name.clone() }),
        Builtin::OutputPin { name } => B::output_pin(pb::Pin { name: name.clone() }),
        Builtin::Coupler { coupling } => B::coupler(pb::Coupler { coupling: *coupling }),
        Builtin::PhaseShifter { phase } => B::phase_shifter(pb::PhaseShifter { phase: *phase }),
        Builtin::Attenuator { loss_db } => B::attenuator(pb::Attenuator { loss_db: *loss_db }),
        Builtin::DelayLine { length_um, n_eff, n_g, reference_nm, loss_db_per_cm } => B::delay_line(pb::DelayLine {
            length_um: *length_um,
            n_eff: *n_eff,
            n_g: *n_g,
            reference_nm: *reference_nm,
            loss_db_per_cm: *loss_db_per_cm,
        }),
        Builtin::Mzi { theta, phi } => B::mzi(pb::Mzi { theta: *theta, phi: *phi }),
        Builtin::YSplitter => B::y_splitter(pb::Empty {}),
        Builtin::YCombiner => B::y_combiner(pb::Empty {}),
        Builtin::Crossing => B::crossing(pb::Empty {}),
        Builtin::Ring { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm } => B::ring(ring(*coupling, *length_um, *n_eff, *loss_db_per_cm, *n_g, *reference_nm)),
        Builtin::AddDropRing { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm } => {
            B::add_drop_ring(ring(*coupling, *length_um, *n_eff, *loss_db_per_cm, *n_g, *reference_nm))
        }
        Builtin::SplitterTree { levels } => B::splitter_tree(pb::Tree { levels: *levels }),
        Builtin::SwitchTree { levels, address } => B::switch_tree(pb::SwitchTree { levels: *levels, address: *address }),
        Builtin::Dft { n } => B::dft(pb::Dft { n: *n }),
        Builtin::NumberInput { bits, value } => B::number_input(pb::NumberInput { bits: *bits, value: *value }),
        Builtin::Display { bits } => B::display(pb::Bits { bits: *bits }),
        Builtin::Register { bits } => B::register(pb::Bits { bits: *bits }),
        Builtin::FunctionCell { name, inputs, light_inputs, outputs, light_outputs, wavelength_nm } => B::function_cell(pb::FunctionCell {
            name: name.clone(),
            inputs: *inputs,
            light_inputs: *light_inputs,
            outputs: outputs.clone(),
            light_outputs: light_outputs.clone(),
            wavelength_nm: *wavelength_nm,
        }),
        Builtin::Model(m) => B::model(model(m)),
        Builtin::Value { name, value } => B::value(pb::Value { name: name.clone(), value: *value }),
    };
    pb::Builtin { kind }
}

fn ring(coupling: f64, length_um: f64, n_eff: f64, loss_db_per_cm: f64, n_g: f64, reference_nm: f64) -> pb::Ring {
    pb::Ring { coupling, length_um, n_eff, loss_db_per_cm, n_g, reference_nm }
}

fn model(m: &ModelSpec) -> pb::Model {
    use shine_format::mod_Model::OneOfsource as S;
    pb::Model {
        name: m.name.clone(),
        inputs: m.inputs.clone(),
        outputs: m.outputs.clone(),
        signal_inputs: m.signal_inputs.clone(),
        signal_outputs: m.signal_outputs.clone(),
        source: match &m.source {
            ModelSource::Table { origin, wavelengths_nm, values } => S::table(pb::Table {
                origin: origin.clone(),
                wavelengths_nm: wavelengths_nm.clone(),
                matrices: values.iter().map(|m| pb::Matrix { re_im: m.iter().flatten().copied().collect() }).collect(),
            }),
            ModelSource::Script { code } => S::script(pb::Script { code: code.clone() }),
        },
    }
}

// ---- reading ------------------------------------------------------------------

/// A bundle's documents, with their ids: each file's own if it has one
/// (unique), else the next free one.
pub(crate) fn documents(bundle: pb::Bundle) -> Result<Vec<DocumentFile>, String> {
    let mut used = std::collections::HashSet::new();
    let mut ids: Vec<Option<DocId>> = bundle.files.iter().map(|f| (f.id != 0 && used.insert(f.id)).then_some(DocId(f.id))).collect();
    let mut next = used.iter().max().copied().unwrap_or(0) + 1;
    for id in &mut ids {
        if id.is_none() {
            *id = Some(DocId(next));
            next += 1;
        }
    }
    let id_of: HashMap<String, DocId> = bundle.files.iter().zip(&ids).map(|(f, id)| (f.path.clone(), id.unwrap())).collect();
    // A box whose component isn't in the project refers to an id no
    // document has: it opens, and simulating it says the component is missing.
    let missing = std::cell::RefCell::new(HashMap::<String, DocId>::new());
    bundle
        .files
        .into_iter()
        .zip(ids)
        .map(|(f, id)| {
            let here = folder_of(&f.path).to_owned();
            let reference = |path: &str| -> DocId {
                let full = resolve(&here, path).unwrap_or_else(|| path.to_owned());
                if let Some(&id) = id_of.get(&full) {
                    return id;
                }
                let mut missing = missing.borrow_mut();
                let n = missing.len() as u64;
                *missing.entry(full).or_insert_with(|| {
                    tracing::warn!("{}: uses “{path}”, which isn't in the project", f.path);
                    DocId(u64::MAX - n)
                })
            };
            let d = f.document.unwrap_or_default();
            Ok(DocumentFile {
                id: id.unwrap(),
                name: d.name,
                component: d.component.map(component_meta),
                cell_um: d.cell_um,
                layout: read_layout(d.layout.unwrap_or_default(), &reference)?,
            })
        })
        .collect()
}

fn component_meta(c: pb::ComponentInfo) -> ComponentMeta {
    let a = c.appearance.unwrap_or_default();
    ComponentMeta {
        initials: c.initials,
        description: c.description,
        appearance: Appearance {
            color: a.color.map(read_color),
            shape: match a.shape {
                pb::BoxShape::RECTANGLE => BoxShape::Rectangle,
                pb::BoxShape::ROUNDED => BoxShape::Rounded,
                pb::BoxShape::CAPSULE => BoxShape::Capsule,
                pb::BoxShape::CHAMFERED => BoxShape::Chamfered,
            },
            icon: a.icon.map(|i| {
                let format = match i.format {
                    pb::IconFormat::WEBP => IconFormat::WebP,
                    pb::IconFormat::SVG => IconFormat::Svg,
                };
                Icon::new(format, i.data)
            }),
        },
    }
}

fn read_color(c: pb::Color) -> [u8; 3] {
    let [_, r, g, b] = c.rgb.to_be_bytes();
    [r, g, b]
}

fn read_point(p: Option<pb::Point>) -> Pos2 {
    p.map_or(Pos2::ZERO, |p| Pos2::new(p.x, p.y))
}

fn read_vector(p: Option<pb::Point>) -> Vec2 {
    p.map_or(Vec2::ZERO, |p| Vec2::new(p.x, p.y))
}

fn read_port(p: Option<pb::PortRef>) -> PortRef {
    let p = p.unwrap_or_default();
    PortRef { instance: InstanceId(p.box_pb), port: p.port as usize }
}

fn read_item(i: Option<pb::Item>) -> Result<Item, String> {
    match i.map(|i| i.item) {
        Some(OneOfitem::part(k)) => Ok(Item::Part(k as usize)),
        Some(OneOfitem::box_pb(id)) => Ok(Item::Box(InstanceId(id))),
        _ => Err("a wire or export without its part or box".into()),
    }
}

fn read_terminal(t: Option<pb::Terminal>) -> Result<Terminal, String> {
    let t = t.unwrap_or_default();
    Ok(Terminal { item: read_item(t.item)?, port: t.port as usize })
}

fn read_layout(l: pb::Layout, reference: &dyn Fn(&str) -> DocId) -> Result<Layout, String> {
    Ok(Layout {
        components: l.parts.into_iter().map(read_part).collect::<Result<_, _>>()?,
        lines: l.lines.into_iter().map(|l| Line { start: read_point(l.start), end: read_point(l.end) }).collect(),
        instances: l.boxes.into_iter().map(|i| read_instance(i, reference)).collect::<Result<_, _>>()?,
        wires: l
            .waveguides
            .into_iter()
            .map(|w| Wire { from: read_port(w.from), to: read_port(w.to), length_um: w.length.map(|l| l.um) })
            .collect(),
        signals: l
            .signals
            .into_iter()
            .map(|s| Ok(SignalWire { from: read_terminal(s.from)?, to: read_terminal(s.to)? }))
            .collect::<Result<_, String>>()?,
        next_instance: l.next_box,
        labels: l.labels.into_iter().map(|l| (l.part as usize, l.text)).collect(),
        feeds: l.feeds.into_iter().map(|f| Feed { laser: f.laser as usize, to: read_port(f.to) }).collect(),
        exports: l
            .exports
            .into_iter()
            .map(|e| Ok(Export { name: e.name, item: read_item(e.item)?, param: e.parameter }))
            .collect::<Result<_, String>>()?,
    })
}

fn read_part(p: pb::Part) -> Result<PlacedComponent, String> {
    use OpticComponentKind as K;
    let kind = match p.kind {
        P::laser(l) => K::Laser {
            amplitude: l.amplitude,
            phase: l.phase,
            wavelength: l.wavelength_nm,
            dir: read_vector(l.direction),
            source: l.source.min(u8::MAX as u32) as u8,
            color: l.color.map(read_color),
        },
        P::beam_splitter(b) => K::BeamSplitter { reflectivity: b.reflectivity, transparency: b.transparency, normal: read_vector(b.normal) },
        P::mirror(m) => K::Mirror { normal: read_vector(m.normal) },
        P::lens(_) => K::Lens,
        P::phase_shifter(p) => K::PhaseShifter { phase: p.phase },
        P::attenuator(a) => K::Attenuator { transmission: a.transmission },
        P::detector(d) => K::Detector { threshold: d.threshold },
        P::input_port(i) => K::InputPort { dir: read_vector(i.direction) },
        P::output_port(_) => K::OutputPort,
        P::signal_in(_) => K::SignalIn,
        P::signal_out(_) => K::SignalOut,
        P::None => return Err("a part of a kind this version of Shine doesn't know; update Shine".into()),
    };
    Ok(PlacedComponent { kind, pos: read_point(p.position) })
}

fn read_instance(i: pb::Instance, reference: &dyn Fn(&str) -> DocId) -> Result<Instance, String> {
    let block = match i.block {
        OneOfblock::builtin(b) => BlockRef::Builtin(read_builtin(b)?),
        OneOfblock::component(path) => BlockRef::Component(reference(&path)),
        OneOfblock::None => return Err("a box of a kind this version of Shine doesn't know; update Shine".into()),
    };
    Ok(Instance {
        id: InstanceId(i.id),
        block,
        cell: (i.column, i.row),
        params: i.parameters.into_iter().map(|p| (p.name, p.value)).collect(),
    })
}

fn read_builtin(b: pb::Builtin) -> Result<Builtin, String> {
    Ok(match b.kind {
        B::laser(l) => Builtin::Laser { amplitude: l.amplitude, phase: l.phase, wavelength_nm: l.wavelength_nm },
        B::detector(_) => Builtin::Detector,
        B::input_pin(p) => Builtin::InputPin { name: p.name },
        B::output_pin(p) => Builtin::OutputPin { name: p.name },
        B::coupler(c) => Builtin::Coupler { coupling: c.coupling },
        B::phase_shifter(p) => Builtin::PhaseShifter { phase: p.phase },
        B::attenuator(a) => Builtin::Attenuator { loss_db: a.loss_db },
        B::delay_line(d) => Builtin::DelayLine {
            length_um: d.length_um,
            n_eff: d.n_eff,
            n_g: d.n_g,
            reference_nm: d.reference_nm,
            loss_db_per_cm: d.loss_db_per_cm,
        },
        B::mzi(m) => Builtin::Mzi { theta: m.theta, phi: m.phi },
        B::y_splitter(_) => Builtin::YSplitter,
        B::y_combiner(_) => Builtin::YCombiner,
        B::crossing(_) => Builtin::Crossing,
        B::ring(r) => Builtin::Ring {
            coupling: r.coupling,
            length_um: r.length_um,
            n_eff: r.n_eff,
            loss_db_per_cm: r.loss_db_per_cm,
            n_g: r.n_g,
            reference_nm: r.reference_nm,
        },
        B::add_drop_ring(r) => Builtin::AddDropRing {
            coupling: r.coupling,
            length_um: r.length_um,
            n_eff: r.n_eff,
            loss_db_per_cm: r.loss_db_per_cm,
            n_g: r.n_g,
            reference_nm: r.reference_nm,
        },
        B::splitter_tree(t) => Builtin::SplitterTree { levels: t.levels },
        B::switch_tree(t) => Builtin::SwitchTree { levels: t.levels, address: t.address },
        B::dft(d) => Builtin::Dft { n: d.n },
        B::number_input(n) => Builtin::NumberInput { bits: n.bits, value: n.value },
        B::display(d) => Builtin::Display { bits: d.bits },
        B::register(r) => Builtin::Register { bits: r.bits },
        B::function_cell(f) => Builtin::FunctionCell {
            name: f.name,
            inputs: f.inputs,
            light_inputs: f.light_inputs,
            outputs: f.outputs,
            light_outputs: f.light_outputs,
            wavelength_nm: f.wavelength_nm,
        },
        B::model(m) => Builtin::Model(Arc::new(read_model(m)?)),
        B::value(v) => Builtin::Value { name: v.name, value: v.value },
        B::None => return Err("a built-in block this version of Shine doesn't know; update Shine".into()),
    })
}

fn read_model(m: pb::Model) -> Result<ModelSpec, String> {
    use shine_format::mod_Model::OneOfsource as S;
    let source = match m.source {
        S::table(t) => ModelSource::Table {
            origin: t.origin,
            wavelengths_nm: t.wavelengths_nm,
            values: t.matrices.into_iter().map(|m| m.re_im.as_chunks::<2>().0.to_vec()).collect(),
        },
        S::script(s) => ModelSource::Script { code: s.code },
        S::None => return Err("a model block of a kind this version of Shine doesn't know; update Shine".into()),
    };
    let mut spec = ModelSpec::new(m.name, m.inputs, m.outputs, source);
    spec.signal_inputs = m.signal_inputs;
    spec.signal_outputs = m.signal_outputs;
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_relative_and_resolve_back() {
        assert_eq!(relative("", "components/mzi.shinecomp"), "components/mzi.shinecomp");
        assert_eq!(relative("components", "components/mzi.shinecomp"), "mzi.shinecomp");
        assert_eq!(relative("components/meshes", "components/mzi.shinecomp"), "../mzi.shinecomp");
        assert_eq!(relative("components", "chip.shinedesign"), "../chip.shinedesign");
        for (from, to) in [("", "a/b.x"), ("a", "a/b.x"), ("a/c", "a/b.x"), ("d", "a/b.x")] {
            assert_eq!(resolve(from, &relative(from, to)).as_deref(), Some(to), "{from} → {to}");
        }
        assert_eq!(resolve("", "../outside.shinecomp"), None);
    }

    #[test]
    fn names_make_unique_paths() {
        let doc = |name: &str, component: bool| DocumentFile {
            id: DocId(1),
            name: name.into(),
            component: component.then(ComponentMeta::default),
            cell_um: 10.0,
            layout: Layout::default(),
        };
        let docs = [doc("Layer 1 (784→64)", true), doc("Layer 1 (784-64)", true), doc("Chip", false), doc("", false)];
        assert_eq!(
            paths(&docs),
            ["components/layer-1-784-64.shinecomp", "components/layer-1-784-64-2.shinecomp", "chip.shinedesign", "untitled.shinedesign"]
        );
    }
}
