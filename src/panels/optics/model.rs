//! Layout data: components on a square grid, in world coordinates.
//!
//! World units are pixels at 100% zoom; cell `(i, j)` spans
//! `[i * GRID, (i + 1) * GRID)` and components sit at cell centres.

use std::f32::consts::PI;

use egui::{Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};

/// World units per grid cell.
pub const GRID: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum OpticComponentKind {
    Laser {
        amplitude: f32,
        phase: f32,
        wavelength: f32,
        dir: Vec2,
    },
    BeamSplitter {
        reflectivity: f32,
        transparency: f32,
        normal: Vec2,
    },
    Mirror {
        normal: Vec2,
    },
    Lens,
    PhaseShifter {
        phase: f32,
    },
    Attenuator {
        /// Power transmission in `[0, 1]`.
        transmission: f32,
    },
    Detector,
    /// Boundary input when the layout is used as a component; light enters
    /// here travelling along `dir`.
    InputPort {
        dir: Vec2,
    },
    /// Boundary output when the layout is used as a component.
    OutputPort,
}

impl OpticComponentKind {
    /// Every component with its default parameters, in palette order.
    pub fn palette() -> [Self; 9] {
        let diagonal = Vec2::new(-1.0, -1.0).normalized();
        [
            Self::Laser {
                amplitude: 1.0,
                phase: 0.0,
                wavelength: 650.0,
                dir: Vec2::new(1.0, 0.0),
            },
            Self::BeamSplitter {
                reflectivity: 0.5,
                transparency: 0.5,
                normal: diagonal,
            },
            Self::Mirror { normal: diagonal },
            Self::Lens,
            Self::PhaseShifter { phase: 0.0 },
            Self::Attenuator { transmission: 0.5 },
            Self::Detector,
            Self::InputPort {
                dir: Vec2::new(1.0, 0.0),
            },
            Self::OutputPort,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Laser { .. } => "Laser",
            Self::BeamSplitter { .. } => "Beam Splitter",
            Self::Mirror { .. } => "Mirror",
            Self::Lens => "Lens",
            Self::PhaseShifter { .. } => "Phase Shifter",
            Self::Attenuator { .. } => "Attenuator",
            Self::Detector => "Detector",
            Self::InputPort { .. } => "Input port",
            Self::OutputPort => "Output port",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Laser { .. } => "🔦",
            Self::BeamSplitter { .. } => "◩",
            Self::Mirror { .. } => "🪞",
            Self::Lens => "🔍",
            Self::PhaseShifter { .. } => "φ",
            Self::Attenuator { .. } => "▒",
            Self::Detector => "◉",
            Self::InputPort { .. } => "⇥",
            Self::OutputPort => "⇤",
        }
    }

    /// Rotate by `steps` increments: 90 degrees for lasers, 45 degrees for
    /// mirrors and splitters. Positive is clockwise on screen.
    pub fn rotate(&mut self, steps: i32) {
        let turn = |v: &mut Vec2, angle: f32| {
            let a = v.y.atan2(v.x) + angle * steps as f32;
            *v = Vec2::new(a.cos(), a.sin());
        };
        match self {
            Self::Laser { dir, .. } | Self::InputPort { dir } => turn(dir, PI / 2.0),
            Self::BeamSplitter { normal, .. } | Self::Mirror { normal } => turn(normal, PI / 4.0),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedComponent {
    pub kind: OpticComponentKind,
    pub pos: Pos2,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub start: Pos2,
    pub end: Pos2,
}

/// Cell index containing a world position.
pub fn cell_of(p: Pos2) -> (i32, i32) {
    ((p.x / GRID).floor() as i32, (p.y / GRID).floor() as i32)
}

pub fn cell_center(i: i32, j: i32) -> Pos2 {
    Pos2::new((i as f32 + 0.5) * GRID, (j as f32 + 0.5) * GRID)
}

/// Centre of the cell containing `p`.
pub fn snap(p: Pos2) -> Pos2 {
    let (i, j) = cell_of(p);
    cell_center(i, j)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub components: Vec<PlacedComponent>,
    pub lines: Vec<Line>,
}

impl Layout {
    /// Component occupying the cell that contains `p`.
    pub fn component_at(&self, p: Pos2) -> Option<usize> {
        let cell = cell_of(p);
        self.components.iter().position(|c| cell_of(c.pos) == cell)
    }

    /// Bounding box of the given components' cells.
    pub fn bounds(&self, indices: impl IntoIterator<Item = usize>) -> Option<Rect> {
        indices
            .into_iter()
            .filter_map(|i| self.components.get(i))
            .map(|c| Rect::from_center_size(c.pos, Vec2::splat(GRID)))
            .reduce(|a, b| a.union(b))
    }

    pub fn all_bounds(&self) -> Option<Rect> {
        self.bounds(0..self.components.len())
    }
}

/// Version of the saved file format.
pub const LAYOUT_VERSION: u32 = 1;

/// On-disk layout. Version 0 files (no `version` field) stored positions in
/// screen space with an arbitrary offset; they are snapped onto the grid.
#[derive(Serialize, Deserialize)]
struct LayoutFile {
    #[serde(default)]
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cell_um: Option<f64>,
    components: Vec<PlacedComponent>,
    lines: Vec<Line>,
}

pub fn to_json(layout: &Layout, cell_um: f64) -> String {
    serde_json::to_string_pretty(&LayoutFile {
        version: LAYOUT_VERSION,
        cell_um: Some(cell_um),
        components: layout.components.clone(),
        lines: layout.lines.clone(),
    })
    .expect("layout is always serialisable")
}

/// Parse a saved layout and the cell size it was saved with, if any.
pub fn from_json(json: &str) -> Result<(Layout, Option<f64>), String> {
    let file: LayoutFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if file.version > LAYOUT_VERSION {
        return Err(format!(
            "file format version {} is newer than supported ({LAYOUT_VERSION})",
            file.version
        ));
    }
    let mut layout = Layout {
        components: file.components,
        lines: file.lines,
    };
    if file.version == 0 {
        for c in &mut layout.components {
            c.pos = snap(c.pos);
        }
        for l in &mut layout.lines {
            l.start = snap(l.start);
            l.end = snap(l.end);
        }
    }
    Ok((layout, file.cell_um))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn grid_helpers() {
        assert_eq!(cell_of(Pos2::new(0.0, 0.0)), (0, 0));
        assert_eq!(cell_of(Pos2::new(39.9, 40.0)), (0, 1));
        assert_eq!(cell_of(Pos2::new(-0.1, -40.0)), (-1, -1));
        assert_eq!(cell_center(-1, 2), Pos2::new(-20.0, 100.0));
        assert_eq!(snap(Pos2::new(41.0, -3.0)), Pos2::new(60.0, -20.0));
        assert_eq!(snap(snap(Pos2::new(7.0, 7.0))), Pos2::new(20.0, 20.0));
    }

    #[test]
    fn palette_names_and_icons_are_unique() {
        let palette = OpticComponentKind::palette();
        let names: std::collections::HashSet<_> = palette.iter().map(|k| k.name()).collect();
        let icons: std::collections::HashSet<_> = palette.iter().map(|k| k.icon()).collect();
        assert_eq!(names.len(), palette.len());
        assert_eq!(icons.len(), palette.len());
    }

    #[test]
    fn rotation_steps() {
        let [mut laser, mut splitter, mut mirror, ..] = OpticComponentKind::palette();
        laser.rotate(1);
        let OpticComponentKind::Laser { dir, .. } = laser else {
            unreachable!()
        };
        assert!(close(dir, Vec2::new(0.0, 1.0)));
        laser.rotate(-1);
        let OpticComponentKind::Laser { dir, .. } = laser else {
            unreachable!()
        };
        assert!(close(dir, Vec2::new(1.0, 0.0)));

        let original = mirror;
        mirror.rotate(2);
        let OpticComponentKind::Mirror { normal } = mirror else {
            unreachable!()
        };
        assert!(close(normal, Vec2::new(1.0, -1.0).normalized()));
        mirror.rotate(6);
        assert!(matches!(mirror, OpticComponentKind::Mirror { normal } if {
            let OpticComponentKind::Mirror { normal: n0 } = original else { unreachable!() };
            close(normal, n0)
        }));

        splitter.rotate(-1);
        let OpticComponentKind::BeamSplitter { normal, .. } = splitter else {
            unreachable!()
        };
        assert!(close(normal, Vec2::new(-1.0, 0.0)));

        for mut kind in OpticComponentKind::palette().into_iter().skip(3).take(4) {
            let before = kind;
            kind.rotate(3);
            assert_eq!(kind, before);
        }
        let mut port = OpticComponentKind::palette()[7];
        port.rotate(2);
        let OpticComponentKind::InputPort { dir } = port else {
            unreachable!()
        };
        assert!(close(dir, Vec2::new(-1.0, 0.0)));
    }

    #[test]
    fn component_lookup_and_bounds() {
        let layout = Layout {
            components: vec![
                PlacedComponent {
                    kind: OpticComponentKind::Lens,
                    pos: cell_center(0, 0),
                },
                PlacedComponent {
                    kind: OpticComponentKind::Detector,
                    pos: cell_center(3, -2),
                },
            ],
            lines: Vec::new(),
        };
        assert_eq!(layout.component_at(Pos2::new(5.0, 35.0)), Some(0));
        assert_eq!(
            layout.component_at(cell_center(3, -2) + Vec2::splat(19.0)),
            Some(1)
        );
        assert_eq!(layout.component_at(cell_center(1, 0)), None);
        assert_eq!(
            layout.all_bounds(),
            Some(Rect::from_min_max(
                Pos2::new(0.0, -80.0),
                Pos2::new(160.0, 40.0)
            ))
        );
        assert_eq!(
            layout.bounds([1, 99]),
            Some(Rect::from_min_size(
                Pos2::new(120.0, -80.0),
                Vec2::splat(GRID)
            ))
        );
        assert_eq!(Layout::default().all_bounds(), None);
    }

    #[test]
    fn json_round_trip_keeps_everything() {
        let layout = Layout {
            components: OpticComponentKind::palette()
                .into_iter()
                .enumerate()
                .map(|(i, kind)| PlacedComponent {
                    kind,
                    pos: cell_center(i as i32, -1),
                })
                .collect(),
            lines: vec![Line {
                start: cell_center(0, 0),
                end: cell_center(2, 2),
            }],
        };
        let (back, cell) = from_json(&to_json(&layout, 2.5)).unwrap();
        assert_eq!(back, layout);
        assert_eq!(cell, Some(2.5));
    }

    #[test]
    fn version_zero_files_are_snapped_to_the_grid() {
        // Saved by the first editor: positions carried the canvas offset.
        let json = r#"{
            "components": [
                {"kind": {"Mirror": {"normal": {"x": -0.7071, "y": -0.7071}}}, "pos": {"x": 217.5, "y": 102.0}},
                {"kind": "Lens", "pos": {"x": 257.5, "y": 102.0}}
            ],
            "lines": [{"start": {"x": 217.5, "y": 102.0}, "end": {"x": 297.5, "y": 142.0}}]
        }"#;
        let (layout, cell) = from_json(json).unwrap();
        assert_eq!(cell, None);
        assert_eq!(layout.components[0].pos, cell_center(5, 2));
        assert_eq!(layout.components[1].pos, cell_center(6, 2));
        assert_eq!(layout.lines[0].end, cell_center(7, 3));
    }

    #[test]
    fn rejects_garbage_and_future_versions() {
        assert!(from_json("not json").is_err());
        let future = r#"{"version": 99, "components": [], "lines": []}"#;
        assert!(from_json(future).unwrap_err().contains("newer"));
    }
}
