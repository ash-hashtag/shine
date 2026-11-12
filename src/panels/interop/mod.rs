//! Files for other tools: S-parameters (Touchstone) for circuit simulators,
//! and netlists with placements for layout tools that make the mask.

pub mod gdsfactory;
pub mod sax;
pub mod touchstone;

use crate::panels::circuit::sim::{self, Machine};
use crate::panels::project::{DocId, Project, layout_interface};

/// `points` wavelengths evenly from `from` to `to` nm.
pub fn wavelengths(from: f64, to: f64, points: usize) -> Vec<f64> {
    let n = points.max(2);
    (0..n).map(|k| from + (to - from) * k as f64 / (n - 1) as f64).collect()
}

/// A document's S-parameters as a Touchstone file: its text and port count
/// (the `N` of `.sNp`).
pub fn touchstone(project: &Project, doc: DocId, wavelengths_nm: &[f64], source: &str) -> Result<(String, usize), String> {
    let d = project.doc(doc).ok_or("no such document")?;
    let t = sim::transmission(d.layout(), d.cell_um(), project, &Machine::default(), Some(doc), wavelengths_nm)?;
    let names = layout_interface(d.layout());
    let s = touchstone::SParameters::from_transmission(&t.wavelengths_nm, &t.matrices);
    let (from, to) = wavelengths_nm.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &w| (a.min(w), b.max(w)));
    let mut comments = vec![
        format!("“{}” from Shine {}{}", d.name, env!("CARGO_PKG_VERSION"), if source.is_empty() { String::new() } else { format!(" ({source})") }),
        format!("S-parameters from {from} to {to} nm ({} points), as fields (real, imaginary).", s.frequencies_hz.len()),
        "Light is one-way in Shine: inputs come first, then outputs; S[output][input] is".into(),
        "the transmission and every other entry is 0 (no reflections or backward light).".into(),
    ];
    let inputs = names.inputs.len();
    for (k, n) in names.inputs.iter().enumerate() {
        comments.push(format!("Port {}: {n} (light input)", k + 1));
    }
    for (k, n) in names.outputs.iter().enumerate() {
        comments.push(format!("Port {}: {n} (light output)", inputs + k + 1));
    }
    Ok((s.to_touchstone(&comments), s.ports))
}

/// The document worth exporting when none is named: the first with light
/// ports, preferring layouts over components.
pub fn default_doc(project: &Project) -> Option<DocId> {
    let with_ports = |d: &&crate::panels::project::Document| {
        let i = d.interface();
        !i.inputs.is_empty() && !i.outputs.is_empty()
    };
    let docs = &project.documents;
    docs.iter()
        .filter(|d| d.component.is_none())
        .find(with_ports)
        .or_else(|| docs.iter().find(with_ports))
        .or_else(|| docs.iter().find(|d| d.component.is_none()))
        .map(|d| d.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::optics::model::{BlockRef, Builtin, Layout, PortRef};

    #[test]
    fn a_couplers_s_parameters_are_its_split() {
        let mut p = Project::new();
        let mut l = Layout::default();
        let a = l.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "a".into() }), (0, 0));
        let b = l.add_instance(BlockRef::Builtin(Builtin::InputPin { name: "b".into() }), (0, 1));
        let dc = l.add_instance(BlockRef::Builtin(Builtin::Coupler { coupling: 0.3 }), (3, 0));
        let bar = l.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "bar".into() }), (7, 0));
        let cross = l.add_instance(BlockRef::Builtin(Builtin::OutputPin { name: "cross".into() }), (7, 1));
        let pr = |instance, port| PortRef { instance, port };
        l.connect_optical(pr(a, 0), pr(dc, 0));
        l.connect_optical(pr(b, 0), pr(dc, 1));
        l.connect_optical(pr(dc, 0), pr(bar, 0));
        l.connect_optical(pr(dc, 1), pr(cross, 0));
        let id = p.add_layout("Coupler", l, 10.0);
        assert_eq!(default_doc(&p), Some(id));
        let (text, ports) = touchstone(&p, id, &wavelengths(1540.0, 1560.0, 5), "c.shine").unwrap();
        assert_eq!(ports, 4);
        assert!(text.contains("! Port 1: a (light input)") && text.contains("! Port 4: cross (light output)"), "{text}");
        let s = touchstone::SParameters::parse(&text, 4).unwrap();
        assert_eq!(s.frequencies_hz.len(), 5);
        for m in &s.matrices {
            // Ports: a, b, bar, cross.
            assert!((m[(2, 0)].norm_sqr() - 0.7).abs() < 1e-9, "a → bar");
            assert!((m[(3, 0)].norm_sqr() - 0.3).abs() < 1e-9, "a → cross");
            assert!((m[(3, 1)].norm_sqr() - 0.7).abs() < 1e-9, "b → cross");
            assert_eq!(m[(0, 2)].norm(), 0.0, "nothing backwards");
        }
        // No ports, no S-parameters.
        let empty = p.add_layout("Empty", Layout::default(), 10.0);
        assert!(touchstone(&p, empty, &[1550.0], "").unwrap_err().contains("Input and Output"));
    }
}
