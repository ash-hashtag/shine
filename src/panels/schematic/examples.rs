//! Example schematics, some built from reusable components.

use egui::Pos2;

use super::model::{BlockRef, Builtin, InstanceId, PortRef, Schematic};
use crate::panels::optics::{OpticComponentKind, examples as layouts, model::Layout};
use crate::panels::project::{ComponentMeta, DocId, Project};

pub struct SchematicExample {
    pub name: &'static str,
    pub description: &'static str,
    /// Adds the example's documents to the project; returns the one to open.
    pub build: fn(&mut Project) -> DocId,
}

fn b(block: Builtin) -> BlockRef {
    BlockRef::Builtin(block)
}

fn wire(s: &mut Schematic, from: InstanceId, out: usize, to: InstanceId, input: usize) {
    s.connect(
        PortRef {
            instance: from,
            port: out,
        },
        PortRef {
            instance: to,
            port: input,
        },
    );
}

fn laser(wavelength_nm: f64, phase: f64) -> BlockRef {
    b(Builtin::Laser {
        amplitude: 1.0,
        phase,
        wavelength_nm,
    })
}

pub fn mzi_from_couplers(project: &mut Project) -> DocId {
    let mut s = Schematic::default();
    let l = s.add(laser(1550.0, 0.0), Pos2::new(0.0, 20.0));
    let c1 = s.add(
        b(Builtin::Coupler { coupling: 0.5 }),
        Pos2::new(140.0, 20.0),
    );
    let ps = s.add(
        b(Builtin::PhaseShifter { phase: 0.0 }),
        Pos2::new(280.0, -40.0),
    );
    let c2 = s.add(
        b(Builtin::Coupler { coupling: 0.5 }),
        Pos2::new(420.0, 20.0),
    );
    let d1 = s.add(b(Builtin::Detector), Pos2::new(560.0, -20.0));
    let d2 = s.add(b(Builtin::Detector), Pos2::new(560.0, 60.0));
    wire(&mut s, l, 0, c1, 0);
    wire(&mut s, c1, 0, ps, 0);
    wire(&mut s, ps, 0, c2, 0);
    wire(&mut s, c1, 1, c2, 1);
    wire(&mut s, c2, 0, d1, 0);
    wire(&mut s, c2, 1, d2, 0);
    project.add_schematic("MZI from couplers", s)
}

pub fn add_drop_filter(project: &mut Project) -> DocId {
    let mut s = Schematic::default();
    let l1 = s.add(laser(1550.0, 0.0), Pos2::new(0.0, 0.0));
    let l2 = s.add(laser(1555.0, 0.0), Pos2::new(0.0, 80.0));
    let y = s.add(b(Builtin::YCombiner), Pos2::new(140.0, 30.0));
    let ring = s.add(
        b(Builtin::palette().swap_remove(13)),
        Pos2::new(280.0, 30.0),
    );
    let through = s.add(b(Builtin::Detector), Pos2::new(420.0, 0.0));
    let drop = s.add(b(Builtin::Detector), Pos2::new(420.0, 80.0));
    wire(&mut s, l1, 0, y, 0);
    wire(&mut s, l2, 0, y, 1);
    wire(&mut s, y, 0, ring, 0);
    wire(&mut s, ring, 0, through, 0);
    wire(&mut s, ring, 1, drop, 0);
    project.add_schematic("Add–drop filter", s)
}

/// The free-space Mach–Zehnder layout with ports instead of laser and detectors.
pub fn free_space_mzi_layout() -> Layout {
    let mut layout = layouts::mach_zehnder();
    for c in &mut layout.components {
        c.kind = match c.kind {
            OpticComponentKind::Laser { dir, .. } => OpticComponentKind::InputPort { dir },
            OpticComponentKind::Detector => OpticComponentKind::OutputPort,
            other => other,
        };
    }
    layout
}

pub fn reusable_free_space_mzi(project: &mut Project) -> DocId {
    let component = project.add_layout("Free-space MZI", free_space_mzi_layout(), 10.0);
    project.doc_mut(component).expect("just added").component = Some(ComponentMeta {
        initials: "FSM".into(),
        description: "Mach–Zehnder interferometer built in free space. The phase shifter in its upper arm picks the output.".into(),
    });

    let mut s = Schematic::default();
    let l = s.add(laser(650.0, 0.0), Pos2::new(0.0, 20.0));
    let first = s.add(BlockRef::Component(component), Pos2::new(140.0, 0.0));
    let second = s.add(BlockRef::Component(component), Pos2::new(320.0, 60.0));
    let d1 = s.add(b(Builtin::Detector), Pos2::new(500.0, -40.0));
    let d2 = s.add(b(Builtin::Detector), Pos2::new(500.0, 50.0));
    let d3 = s.add(b(Builtin::Detector), Pos2::new(500.0, 130.0));
    wire(&mut s, l, 0, first, 0);
    wire(&mut s, first, 0, d1, 0);
    wire(&mut s, first, 1, second, 0);
    wire(&mut s, second, 0, d2, 0);
    wire(&mut s, second, 1, d3, 0);
    project.add_schematic("Cascaded free-space MZIs", s)
}

pub fn optical_dft(project: &mut Project) -> DocId {
    let n = 4;
    let tone = 1;
    let mut s = Schematic::default();
    let dft = s.add(b(Builtin::Dft { n: n as u32 }), Pos2::new(200.0, 0.0));
    for j in 0..n {
        let phase = 2.0 * std::f64::consts::PI * (tone * j) as f64 / n as f64;
        let l = s.add(laser(1550.0, phase), Pos2::new(0.0, j as f32 * 60.0 - 20.0));
        wire(&mut s, l, 0, dft, j);
        let d = s.add(
            b(Builtin::Detector),
            Pos2::new(400.0, j as f32 * 60.0 - 20.0),
        );
        wire(&mut s, dft, j, d, 0);
    }
    project.add_schematic("Optical DFT", s)
}

pub fn schematic_examples() -> Vec<SchematicExample> {
    vec![
        SchematicExample {
            name: "Mach–Zehnder from couplers",
            description: "Two directional couplers and a phase shifter. Select the phase shifter and drag its phase to move the light between the detectors.",
            build: mzi_from_couplers,
        },
        SchematicExample {
            name: "Add–drop filter",
            description: "Two lasers at 1550 nm and 1555 nm share a waveguide. The ring is resonant at 1550 nm, so that channel leaves on the drop port while 1555 nm passes through.",
            build: add_drop_filter,
        },
        SchematicExample {
            name: "Reusable free-space component",
            description: "A free-space Mach–Zehnder layout saved as a component (FSM) and used twice. Select one and press Tab to open its layout; change its phase shifter and both instances follow.",
            build: reusable_free_space_mzi,
        },
        SchematicExample {
            name: "Optical DFT",
            description: "Four lasers carry a sampled tone (phases 0, π/2, π, 3π/2). The DFT mesh focuses it onto output 2 — a Fourier transform computed by interference.",
            build: optical_dft,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readings(project: &Project, id: DocId) -> Vec<f64> {
        let sim = project.simulate_schematic(id);
        assert!(sim.error.is_none(), "{:?}", sim.error);
        sim.readings.iter().map(|r| r.1).collect()
    }

    #[test]
    fn catalogue() {
        let examples = schematic_examples();
        assert_eq!(examples.len(), 4);
        for ex in examples {
            let mut p = Project::new();
            let id = (ex.build)(&mut p);
            assert!(p.doc(id).is_some(), "{}", ex.name);
            assert!(ex.description.len() > 40);
        }
    }

    #[test]
    fn mzi_sends_everything_to_one_detector() {
        let mut p = Project::new();
        let id = mzi_from_couplers(&mut p);
        let r = readings(&p, id);
        assert!(r[0] < 1e-12 && (r[1] - 1.0).abs() < 1e-12, "{r:?}");
    }

    #[test]
    fn add_drop_separates_wavelengths() {
        let mut p = Project::new();
        let id = add_drop_filter(&mut p);
        let sim = p.simulate_schematic(id);
        assert_eq!(sim.wavelengths, vec![1550.0, 1555.0]);
        let r = readings(&p, id);
        // Each laser loses half in the Y-combiner; 1550 nm drops, 1555 nm passes.
        assert!(
            (r[0] - 0.5).abs() < 0.02 && (r[1] - 0.5).abs() < 0.02,
            "{r:?}"
        );
    }

    #[test]
    fn reused_component_routes_light() {
        let mut p = Project::new();
        let id = reusable_free_space_mzi(&mut p);
        assert_eq!(p.catalog().components().count(), 1);
        let r = readings(&p, id);
        assert!((r.iter().sum::<f64>() - 1.0).abs() < 1e-9, "{r:?}");
        assert!(r.iter().cloned().fold(0.0, f64::max) > 0.999999);
    }

    #[test]
    fn dft_focuses_tone_on_output_two() {
        let mut p = Project::new();
        let id = optical_dft(&mut p);
        let r = readings(&p, id);
        for (k, v) in r.iter().enumerate() {
            let expected = if k == 1 { 4.0 } else { 0.0 };
            assert!((v - expected).abs() < 1e-9, "{r:?}");
        }
    }
}
