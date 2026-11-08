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
    /// Absorbs light. Its signal output carries the received power, or a
    /// bit (power >= threshold) when `threshold` is above zero.
    Detector {
        threshold: f32,
    },
    /// Boundary input when the layout is used as a component; light enters
    /// here travelling along `dir`.
    InputPort {
        dir: Vec2,
    },
    /// Boundary output when the layout is used as a component.
    OutputPort,
    /// Electrical input of the layout when used as a component.
    SignalIn,
    /// Electrical output of the layout when used as a component.
    SignalOut,
}

/// A detector with an analog (power) output.
pub const DETECTOR: OpticComponentKind = OpticComponentKind::Detector { threshold: 0.0 };

impl OpticComponentKind {
    /// Every component with its default parameters, in palette order.
    pub fn palette() -> [Self; 11] {
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
            DETECTOR,
            Self::InputPort {
                dir: Vec2::new(1.0, 0.0),
            },
            Self::OutputPort,
            Self::SignalIn,
            Self::SignalOut,
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
            Self::Detector { .. } => "Detector",
            Self::InputPort { .. } => "Input port",
            Self::OutputPort => "Output port",
            Self::SignalIn => "Signal in",
            Self::SignalOut => "Signal out",
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
            Self::Detector { .. } => "◉",
            Self::InputPort { .. } => "⇥",
            Self::OutputPort => "⇤",
            Self::SignalIn => "▾",
            Self::SignalOut => "▴",
        }
    }

    /// Electrical inputs: a driven laser scales its amplitude, a phase
    /// shifter adds π per unit, an attenuator scales its transmission.
    pub fn signal_inputs(&self) -> &'static [&'static str] {
        match self {
            Self::Laser { .. } => &["amplitude"],
            Self::PhaseShifter { .. } => &["phase"],
            Self::Attenuator { .. } => &["transmission"],
            Self::SignalOut => &["value"],
            _ => &[],
        }
    }

    pub fn signal_outputs(&self) -> &'static [&'static str] {
        match self {
            Self::Detector { .. } => &["power"],
            Self::SignalIn => &["value"],
            _ => &[],
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

pub use crate::panels::schematic::model::{BlockRef, Builtin, InstanceId, PortRef, Wire};

/// Something with electrical terminals: a primitive (by index into
/// [`Layout::components`]) or a component instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Item {
    Part(usize),
    Box(InstanceId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Terminal {
    pub item: Item,
    pub port: usize,
}

/// An electrical connection from a signal output to a signal input.
/// Unlike light, a signal can drive any number of inputs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalWire {
    pub from: Terminal,
    pub to: Terminal,
}

/// A component placed on the grid as a box, its top-left cell at `cell`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub id: InstanceId,
    pub block: BlockRef,
    pub cell: (i32, i32),
}

/// Ports of a box: light on the left (in) and right (out) edges, one per
/// row; signals on the top (in) and bottom (out) edges, one per column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PortCounts {
    pub optical_in: usize,
    pub optical_out: usize,
    pub signal_in: usize,
    pub signal_out: usize,
}

/// Size of a box in cells (columns, rows).
pub fn box_size(c: PortCounts) -> (i32, i32) {
    (
        c.signal_in.max(c.signal_out).max(2) as i32,
        c.optical_in.max(c.optical_out).max(1) as i32,
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxGeometry {
    pub rect: Rect,
    pub optical_in: Vec<Pos2>,
    pub optical_out: Vec<Pos2>,
    pub signal_in: Vec<Pos2>,
    pub signal_out: Vec<Pos2>,
    /// Cells the box covers.
    pub cells: Vec<(i32, i32)>,
}

pub fn box_geometry(cell: (i32, i32), counts: PortCounts) -> BoxGeometry {
    let (w, h) = box_size(counts);
    let (i, j) = cell;
    let g = GRID;
    let rect = Rect::from_min_size(Pos2::new(i as f32 * g, j as f32 * g), Vec2::new(w as f32 * g, h as f32 * g));
    let row = |k: usize| (j as f32 + k as f32 + 0.5) * g;
    let col = |k: usize| (i as f32 + k as f32 + 0.5) * g;
    BoxGeometry {
        optical_in: (0..counts.optical_in).map(|k| Pos2::new(rect.left(), row(k))).collect(),
        optical_out: (0..counts.optical_out).map(|k| Pos2::new(rect.right(), row(k))).collect(),
        signal_in: (0..counts.signal_in).map(|k| Pos2::new(col(k), rect.top())).collect(),
        signal_out: (0..counts.signal_out).map(|k| Pos2::new(col(k), rect.bottom())).collect(),
        cells: (0..w).flat_map(|di| (0..h).map(move |dj| (i + di, j + dj))).collect(),
        rect,
    }
}

/// Where a primitive's electrical input (top edge) and output (bottom edge) sit.
pub fn part_signal_in(pos: Pos2) -> Pos2 {
    pos - Vec2::new(0.0, GRID / 2.0)
}

pub fn part_signal_out(pos: Pos2) -> Pos2 {
    pos + Vec2::new(0.0, GRID / 2.0)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub components: Vec<PlacedComponent>,
    pub lines: Vec<Line>,
    pub instances: Vec<Instance>,
    /// Waveguides between component ports.
    pub wires: Vec<Wire>,
    pub signals: Vec<SignalWire>,
    pub next_instance: u32,
    /// Names of port and pin parts (by part index), shown on the component's box.
    pub labels: Vec<(usize, String)>,
}

impl Layout {
    pub fn with_components(components: Vec<PlacedComponent>) -> Self {
        Self {
            components,
            ..Self::default()
        }
    }

    pub fn label(&self, part: usize) -> Option<&str> {
        self.labels.iter().find(|(i, _)| *i == part).map(|(_, l)| l.as_str())
    }

    pub fn set_label(&mut self, part: usize, label: &str) {
        self.labels.retain(|(i, _)| *i != part);
        if !label.trim().is_empty() {
            self.labels.push((part, label.trim().to_owned()));
        }
    }

    pub fn add_instance(&mut self, block: BlockRef, cell: (i32, i32)) -> InstanceId {
        let id = InstanceId(self.next_instance);
        self.next_instance += 1;
        self.instances.push(Instance { id, block, cell });
        id
    }

    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    pub fn instance_index(&self, id: InstanceId) -> Option<usize> {
        self.instances.iter().position(|i| i.id == id)
    }

    /// Instance covering world position `p`, given each block's port counts.
    pub fn instance_at(&self, p: Pos2, counts: &dyn Fn(&BlockRef) -> PortCounts) -> Option<usize> {
        let cell = cell_of(p);
        self.instances
            .iter()
            .rposition(|inst| box_geometry(inst.cell, counts(&inst.block)).cells.contains(&cell))
    }

    /// Whether `cell` is taken by a primitive or a box.
    pub fn occupied(&self, cell: (i32, i32), counts: &dyn Fn(&BlockRef) -> PortCounts) -> bool {
        self.components.iter().any(|c| cell_of(c.pos) == cell)
            || self.instances.iter().any(|inst| box_geometry(inst.cell, counts(&inst.block)).cells.contains(&cell))
    }

    /// Connect a signal output to an input; an input has one driver, so any
    /// wire already into `to` is replaced.
    pub fn connect_signal(&mut self, from: Terminal, to: Terminal) {
        self.signals.retain(|w| w.to != to);
        self.signals.push(SignalWire { from, to });
    }

    /// Driver of a signal input, if wired.
    pub fn signal_source(&self, to: Terminal) -> Option<Terminal> {
        self.signals.iter().find(|w| w.to == to).map(|w| w.from)
    }

    /// Connect two box ports with a waveguide. Light ports take one
    /// waveguide each, so wires on either end are replaced.
    pub fn connect_optical(&mut self, from: PortRef, to: PortRef) {
        self.wires.retain(|w| w.from != from && w.to != to);
        self.wires.push(Wire {
            from,
            to,
            length_um: None,
        });
    }

    /// Remove primitives by index, with their signal wires; later indices shift down.
    pub fn remove_parts(&mut self, indices: &[usize]) {
        let mut doomed: Vec<usize> = indices.iter().copied().filter(|&i| i < self.components.len()).collect();
        doomed.sort_unstable();
        doomed.dedup();
        for &i in doomed.iter().rev() {
            self.components.remove(i);
        }
        self.labels = std::mem::take(&mut self.labels)
            .into_iter()
            .filter(|(i, _)| doomed.binary_search(i).is_err())
            .map(|(i, l)| (i - doomed.partition_point(|&d| d < i), l))
            .collect();
        let remap = |item: Item| match item {
            Item::Part(i) if doomed.binary_search(&i).is_ok() => None,
            Item::Part(i) => Some(Item::Part(i - doomed.partition_point(|&d| d < i))),
            other => Some(other),
        };
        self.signals = std::mem::take(&mut self.signals)
            .into_iter()
            .filter_map(|w| {
                Some(SignalWire {
                    from: Terminal { item: remap(w.from.item)?, port: w.from.port },
                    to: Terminal { item: remap(w.to.item)?, port: w.to.port },
                })
            })
            .collect();
    }

    /// Remove instances, with every waveguide and signal wire touching them.
    pub fn remove_instances(&mut self, ids: &[InstanceId]) {
        self.instances.retain(|i| !ids.contains(&i.id));
        self.wires.retain(|w| !ids.contains(&w.from.instance) && !ids.contains(&w.to.instance));
        let touches = |t: &Terminal| matches!(t.item, Item::Box(id) if ids.contains(&id));
        self.signals.retain(|w| !touches(&w.from) && !touches(&w.to));
    }

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

/// Last JSON layout version written by earlier releases.
pub const LAYOUT_VERSION: u32 = 1;

/// Layout files saved as JSON by earlier versions. Version 0 files (no
/// `version` field) stored positions in screen space with an arbitrary
/// offset; they are snapped onto the grid. New files use the binary project
/// format instead.
#[derive(Deserialize)]
struct LayoutFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    cell_um: Option<f64>,
    components: Vec<PlacedComponent>,
    lines: Vec<Line>,
}

/// Import a layout saved as JSON by an earlier version, and its cell size if recorded.
pub fn from_legacy_json(json: &str) -> Result<(Layout, Option<f64>), String> {
    let mut value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    // Detectors were a plain variant before they gained a threshold.
    if let Some(parts) = value.get_mut("components").and_then(|c| c.as_array_mut()) {
        for part in parts {
            if part.get("kind").and_then(|k| k.as_str()) == Some("Detector") {
                part["kind"] = serde_json::json!({"Detector": {"threshold": 0.0}});
            }
        }
    }
    let file: LayoutFile = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if file.version > LAYOUT_VERSION {
        return Err(format!(
            "file format version {} is newer than supported ({LAYOUT_VERSION})",
            file.version
        ));
    }
    let mut layout = Layout {
        components: file.components,
        lines: file.lines,
        ..Layout::default()
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
                    kind: crate::panels::optics::model::DETECTOR,
                    pos: cell_center(3, -2),
                },
            ],
            ..Layout::default()
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
    fn imports_version_one_json() {
        let json = r#"{"version": 1, "cell_um": 2.5, "components": [
            {"kind": {"PhaseShifter": {"phase": 1.0}}, "pos": {"x": 20.0, "y": 60.0}}], "lines": []}"#;
        let (layout, cell) = from_legacy_json(json).unwrap();
        assert_eq!(cell, Some(2.5));
        assert_eq!(layout.components[0].pos, cell_center(0, 1));
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
        let (layout, cell) = from_legacy_json(json).unwrap();
        assert_eq!(cell, None);
        assert_eq!(layout.components[0].pos, cell_center(5, 2));
        assert_eq!(layout.components[1].pos, cell_center(6, 2));
        assert_eq!(layout.lines[0].end, cell_center(7, 3));
    }

    #[test]
    fn rejects_garbage_and_future_versions() {
        assert!(from_legacy_json("not json").is_err());
        let future = r#"{"version": 99, "components": [], "lines": []}"#;
        assert!(from_legacy_json(future).unwrap_err().contains("newer"));
    }
}
