//! Editing operations on a [`Layout`], independent of any UI.
//!
//! The selection covers primitives (by index) and boxes (by instance
//! index). Operations keep wires consistent: deleting removes the wires
//! that touched what was deleted, duplicating copies the wires between the
//! copied items.

use std::collections::{HashMap, HashSet};

use egui::{Pos2, Rect, Vec2};

use super::model::{
    BlockRef, Feed, GRID, Instance, Item, Layout, OpticComponentKind, PlacedComponent, PortCounts, PortRef, SignalWire, Terminal, Wire,
    box_geometry, cell_of, snap,
};
pub use crate::panels::history::History;
pub use crate::panels::selection::Selection;

/// Port counts of a block (needed to know how much room a box takes).
pub type Counts<'a> = &'a dyn Fn(&BlockRef) -> PortCounts;

/// For layouts without boxes, and in tests.
pub fn no_ports(_: &BlockRef) -> PortCounts {
    PortCounts::default()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    #[default]
    Free,
    X,
    Y,
}

/// An in-progress move of the selection.
#[derive(Clone, Debug, PartialEq)]
pub struct Grab {
    pub start: Pos2,
    /// Selected primitives and where they started.
    pub originals: Vec<(usize, Pos2)>,
    /// Selected boxes (instance indices) and their starting cells.
    pub box_originals: Vec<(usize, (i32, i32))>,
    pub axis: Axis,
    /// Started by Shift+D: what was copied. Cancelling removes the copies and
    /// re-selects these.
    pub duplicated_from: Option<(Vec<usize>, Vec<usize>)>,
    /// Started by dragging with the mouse: releasing the button confirms.
    pub tweak: bool,
    /// Whole cells moved so far.
    pub offset: (i32, i32),
    /// The last pointer position the grab followed, so arrow keys can move it
    /// on from there.
    pub mouse: Pos2,
}

#[derive(Debug)]
pub struct Editor {
    /// Selected primitives.
    pub selection: Selection,
    /// Selected boxes, by index into [`Layout::instances`].
    pub boxes: Selection,
    pub history: History<Layout>,
    pub grab: Option<Grab>,
}

fn box_cells(layout: &Layout, counts: Counts, skip: Option<usize>) -> HashSet<(i32, i32)> {
    layout
        .instances
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != skip)
        .flat_map(|(_, inst)| box_geometry(inst.cell, counts(&inst.block)).cells)
        .collect()
}

impl Editor {
    pub fn new(layout: &Layout) -> Self {
        Self {
            selection: Selection::default(),
            boxes: Selection::default(),
            history: History::new(layout),
            grab: None,
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.boxes.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.selection.is_empty() && self.boxes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.selection.len() + self.boxes.len()
    }

    pub fn select_all(&mut self, layout: &Layout) {
        self.selection.select_all(layout.components.len());
        self.boxes.select_all(layout.instances.len());
    }

    pub fn invert(&mut self, layout: &Layout) {
        self.selection.invert(layout.components.len());
        self.boxes.invert(layout.instances.len());
    }

    /// Left click at `world`. Returns whether a part or box was hit.
    pub fn click_select(&mut self, layout: &Layout, world: Pos2, extend: bool, counts: Counts) -> bool {
        if let Some(i) = layout.component_at(world) {
            if extend {
                self.selection.toggle(i);
            } else {
                self.clear_selection();
                self.selection.select_only(i);
            }
            return true;
        }
        if let Some(b) = layout.instance_at(world, counts) {
            if extend {
                self.boxes.toggle(b);
            } else {
                self.clear_selection();
                self.boxes.select_only(b);
            }
            return true;
        }
        if !extend {
            self.clear_selection();
        }
        false
    }

    /// Select parts whose centres, and boxes whose outlines, touch `area`.
    pub fn box_select(&mut self, layout: &Layout, area: Rect, extend: bool, counts: Counts) {
        if !extend {
            self.clear_selection();
        }
        for (i, c) in layout.components.iter().enumerate() {
            if area.contains(c.pos) {
                self.selection.add(i);
            }
        }
        for (i, inst) in layout.instances.iter().enumerate() {
            if area.intersects(box_geometry(inst.cell, counts(&inst.block)).rect) {
                self.boxes.add(i);
            }
        }
    }

    /// Place a primitive in the cell at `world` unless it is taken.
    pub fn place(&mut self, layout: &mut Layout, kind: OpticComponentKind, world: Pos2, counts: Counts) -> Option<usize> {
        if layout.occupied(cell_of(world), counts) {
            return None;
        }
        layout.components.push(PlacedComponent { kind, pos: snap(world) });
        let i = layout.components.len() - 1;
        self.clear_selection();
        self.selection.select_only(i);
        Some(i)
    }

    /// Place a box with its top-left cell at `world`, unless any of its cells is taken.
    pub fn place_block(&mut self, layout: &mut Layout, block: BlockRef, world: Pos2, counts: Counts) -> Option<usize> {
        let cell = cell_of(world);
        let footprint = box_geometry(cell, counts(&block)).cells;
        if footprint.iter().any(|&c| layout.occupied(c, counts)) {
            return None;
        }
        layout.add_instance(block, cell);
        let i = layout.instances.len() - 1;
        self.clear_selection();
        self.boxes.select_only(i);
        Some(i)
    }

    pub fn delete_selected(&mut self, layout: &mut Layout) -> usize {
        let parts: Vec<usize> = self.selection.iter().collect();
        let ids: Vec<_> = self.boxes.iter().filter_map(|i| layout.instances.get(i).map(|inst| inst.id)).collect();
        layout.remove_parts(&parts);
        layout.remove_instances(&ids);
        self.clear_selection();
        parts.len() + ids.len()
    }

    pub fn rotate_selected(&mut self, layout: &mut Layout, steps: i32) {
        for i in self.selection.iter() {
            layout.components[i].kind.rotate(steps);
        }
    }

    /// Start moving the selection. Returns false when nothing is selected.
    pub fn begin_grab(&mut self, layout: &Layout, mouse: Pos2, tweak: bool) -> bool {
        if self.is_empty() {
            return false;
        }
        self.grab = Some(Grab {
            start: mouse,
            originals: self.selection.iter().map(|i| (i, layout.components[i].pos)).collect(),
            box_originals: self.boxes.iter().map(|i| (i, layout.instances[i].cell)).collect(),
            axis: Axis::Free,
            duplicated_from: None,
            tweak,
            offset: (0, 0),
            mouse,
        });
        true
    }

    /// Move a grab on by whole cells, as if the pointer had moved that far.
    pub fn nudge_grab(&mut self, layout: &mut Layout, cells: (i32, i32)) {
        let Some(g) = &mut self.grab else { return };
        g.start -= Vec2::new(cells.0 as f32, cells.1 as f32) * GRID;
        let mouse = g.mouse;
        self.update_grab(layout, mouse);
    }

    /// Copy the selection (with the wires between copied items) and start
    /// moving the copies.
    pub fn duplicate_selected(&mut self, layout: &mut Layout, mouse: Pos2) -> bool {
        if self.is_empty() {
            return false;
        }
        let parts: Vec<usize> = self.selection.iter().collect();
        let boxes: Vec<usize> = self.boxes.iter().collect();
        let mut item_map: HashMap<Item, Item> = HashMap::new();
        let first_part = layout.components.len();
        for (k, &i) in parts.iter().enumerate() {
            layout.components.push(layout.components[i]);
            item_map.insert(Item::Part(i), Item::Part(first_part + k));
        }
        let first_box = layout.instances.len();
        for &i in &boxes {
            let inst: Instance = layout.instances[i].clone();
            let id = layout.add_instance(inst.block, inst.cell);
            item_map.insert(Item::Box(inst.id), Item::Box(id));
        }
        let map = |t: Terminal| item_map.get(&t.item).map(|&item| Terminal { item, port: t.port });
        let signals: Vec<SignalWire> = layout
            .signals
            .iter()
            .filter_map(|w| Some(SignalWire { from: map(w.from)?, to: map(w.to)? }))
            .collect();
        layout.signals.extend(signals);
        let box_id = |id| match item_map.get(&Item::Box(id)) {
            Some(Item::Box(new)) => Some(*new),
            _ => None,
        };
        let wires: Vec<Wire> = layout
            .wires
            .iter()
            .filter_map(|w| {
                let mut copy = w.clone();
                copy.from.instance = box_id(w.from.instance)?;
                copy.to.instance = box_id(w.to.instance)?;
                Some(copy)
            })
            .collect();
        layout.wires.extend(wires);
        let feeds: Vec<Feed> = layout
            .feeds
            .iter()
            .filter_map(|f| match item_map.get(&Item::Part(f.laser))? {
                Item::Part(laser) => Some(Feed { laser: *laser, to: PortRef { instance: box_id(f.to.instance)?, port: f.to.port } }),
                Item::Box(_) => None,
            })
            .collect();
        layout.feeds.extend(feeds);

        self.clear_selection();
        for i in first_part..layout.components.len() {
            self.selection.add(i);
        }
        for i in first_box..layout.instances.len() {
            self.boxes.add(i);
        }
        self.begin_grab(layout, mouse, false);
        if let Some(g) = &mut self.grab {
            g.duplicated_from = Some((parts, boxes));
        }
        true
    }

    /// Like Blender: pressing the same axis again frees the movement.
    pub fn constrain(&mut self, axis: Axis) {
        if let Some(g) = &mut self.grab {
            g.axis = if g.axis == axis { Axis::Free } else { axis };
        }
    }

    pub fn update_grab(&mut self, layout: &mut Layout, mouse: Pos2) {
        let Some(g) = &mut self.grab else { return };
        g.mouse = mouse;
        let delta = mouse - g.start;
        let mut cells = ((delta.x / GRID).round() as i32, (delta.y / GRID).round() as i32);
        match g.axis {
            Axis::Free => {}
            Axis::X => cells.1 = 0,
            Axis::Y => cells.0 = 0,
        }
        g.offset = cells;
        let shift = Vec2::new(cells.0 as f32, cells.1 as f32) * GRID;
        for &(i, original) in &g.originals {
            layout.components[i].pos = original + shift;
        }
        for &(i, (ci, cj)) in &g.box_originals {
            layout.instances[i].cell = (ci + cells.0, cj + cells.1);
        }
    }

    /// Finish the move. Fails (and reverts) if anything would overlap.
    pub fn confirm_grab(&mut self, layout: &mut Layout, counts: Counts) -> Result<(), &'static str> {
        if self.grab.is_none() {
            return Ok(());
        }
        let mut taken = HashSet::new();
        let mut overlapping = layout.components.iter().any(|c| !taken.insert(cell_of(c.pos)));
        for inst in &layout.instances {
            for cell in box_geometry(inst.cell, counts(&inst.block)).cells {
                overlapping |= !taken.insert(cell);
            }
        }
        let _ = box_cells;
        if overlapping {
            self.cancel_grab(layout);
            return Err("cells already occupied");
        }
        self.grab = None;
        Ok(())
    }

    pub fn cancel_grab(&mut self, layout: &mut Layout) {
        let Some(g) = self.grab.take() else { return };
        if let Some((parts, boxes)) = g.duplicated_from {
            let copies: Vec<usize> = g.originals.iter().map(|o| o.0).collect();
            let ids: Vec<_> = g.box_originals.iter().map(|&(i, _)| layout.instances[i].id).collect();
            layout.remove_parts(&copies);
            layout.remove_instances(&ids);
            self.clear_selection();
            for i in parts {
                self.selection.add(i);
            }
            for i in boxes {
                self.boxes.add(i);
            }
        } else {
            for (i, original) in g.originals {
                layout.components[i].pos = original;
            }
            for (i, cell) in g.box_originals {
                layout.instances[i].cell = cell;
            }
        }
    }

    pub fn undo(&mut self, layout: &mut Layout) -> bool {
        self.cancel_grab(layout);
        let changed = self.history.undo(layout);
        self.selection.retain_valid(layout.components.len());
        self.boxes.retain_valid(layout.instances.len());
        changed
    }

    pub fn redo(&mut self, layout: &mut Layout) -> bool {
        self.cancel_grab(layout);
        let changed = self.history.redo(layout);
        self.selection.retain_valid(layout.components.len());
        self.boxes.retain_valid(layout.instances.len());
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::optics::model::{DETECTOR, cell_center};

    fn layout(cells: &[(i32, i32)]) -> Layout {
        Layout::with_components(
            cells
                .iter()
                .map(|&(i, j)| PlacedComponent {
                    kind: OpticComponentKind::Lens,
                    pos: cell_center(i, j),
                })
                .collect(),
        )
    }

    fn cells(layout: &Layout) -> Vec<(i32, i32)> {
        layout.components.iter().map(|c| cell_of(c.pos)).collect()
    }

    #[test]
    fn click_and_box_select() {
        let doc = layout(&[(0, 0), (1, 0), (5, 5)]);
        let mut e = Editor::new(&doc);
        assert!(e.click_select(&doc, cell_center(1, 0), false, &no_ports));
        assert_eq!(e.selection.iter().collect::<Vec<_>>(), vec![1]);
        assert!(e.click_select(&doc, cell_center(0, 0), true, &no_ports));
        assert_eq!(e.selection.len(), 2);
        // Shift-click on empty space keeps the selection; plain click clears it.
        assert!(!e.click_select(&doc, cell_center(9, 9), true, &no_ports));
        assert_eq!(e.selection.len(), 2);
        e.click_select(&doc, cell_center(9, 9), false, &no_ports);
        assert!(e.selection.is_empty());

        let area = Rect::from_min_max(Pos2::new(-10.0, -10.0), Pos2::new(70.0, 30.0));
        e.box_select(&doc, area, false, &no_ports);
        assert_eq!(e.selection.iter().collect::<Vec<_>>(), vec![0, 1]);
        let far = Rect::from_center_size(cell_center(5, 5), Vec2::splat(10.0));
        e.box_select(&doc, far, true, &no_ports);
        assert_eq!(e.selection.len(), 3);
        e.box_select(&doc, far, false, &no_ports);
        assert_eq!(e.selection.iter().collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn place_rejects_occupied_cells() {
        let mut doc = layout(&[(0, 0)]);
        let mut e = Editor::new(&doc);
        assert_eq!(
            e.place(&mut doc, DETECTOR, cell_center(0, 0), &no_ports),
            None
        );
        let i = e.place(&mut doc, DETECTOR, Pos2::new(45.0, 3.0), &no_ports);
        assert_eq!(i, Some(1));
        assert_eq!(doc.components[1].pos, cell_center(1, 0));
        assert_eq!(e.selection.active(), Some(1));
    }

    #[test]
    fn delete_and_rotate_selection() {
        let mut doc = layout(&[(0, 0), (1, 0), (2, 0)]);
        doc.components[1].kind = OpticComponentKind::palette()[0];
        let mut e = Editor::new(&doc);
        e.selection.select_only(1);
        e.rotate_selected(&mut doc, 1);
        assert_ne!(doc.components[1].kind, OpticComponentKind::palette()[0]);
        e.selection.add(0);
        assert_eq!(e.delete_selected(&mut doc), 2);
        assert_eq!(cells(&doc), vec![(2, 0)]);
        assert!(e.selection.is_empty());
        assert_eq!(e.delete_selected(&mut doc), 0);
    }

    #[test]
    fn grab_moves_in_whole_cells_with_axis_constraints() {
        let mut doc = layout(&[(0, 0), (1, 0)]);
        let mut e = Editor::new(&doc);
        assert!(!e.begin_grab(&doc, Pos2::ZERO, false), "nothing selected");
        e.selection.select_only(0);
        assert!(e.begin_grab(&doc, Pos2::ZERO, false));

        e.update_grab(&mut doc, Pos2::new(15.0, 0.0));
        assert_eq!(cells(&doc)[0], (0, 0), "less than half a cell");
        e.update_grab(&mut doc, Pos2::new(95.0, 50.0));
        assert_eq!(cells(&doc)[0], (2, 1));
        assert_eq!(e.grab.as_ref().unwrap().offset, (2, 1));

        e.constrain(Axis::X);
        e.update_grab(&mut doc, Pos2::new(95.0, 50.0));
        assert_eq!(cells(&doc)[0], (2, 0));
        e.constrain(Axis::Y);
        e.update_grab(&mut doc, Pos2::new(95.0, 50.0));
        assert_eq!(cells(&doc)[0], (0, 1));
        e.constrain(Axis::Y);
        assert_eq!(e.grab.as_ref().unwrap().axis, Axis::Free);

        e.update_grab(&mut doc, Pos2::new(0.0, 85.0));
        assert!(e.confirm_grab(&mut doc, &no_ports).is_ok());
        assert!(e.grab.is_none());
        assert_eq!(cells(&doc), vec![(0, 2), (1, 0)]);
        assert!(e.confirm_grab(&mut doc, &no_ports).is_ok(), "no grab is a no-op");
    }

    #[test]
    fn grab_cancel_and_overlap() {
        let mut doc = layout(&[(0, 0), (1, 0)]);
        let mut e = Editor::new(&doc);
        e.selection.select_only(0);
        e.begin_grab(&doc, Pos2::ZERO, true);
        assert!(e.grab.as_ref().unwrap().tweak);
        e.update_grab(&mut doc, Pos2::new(200.0, 0.0));
        e.cancel_grab(&mut doc);
        assert_eq!(cells(&doc), vec![(0, 0), (1, 0)]);

        e.begin_grab(&doc, Pos2::ZERO, false);
        e.update_grab(&mut doc, Pos2::new(40.0, 0.0));
        assert!(e.confirm_grab(&mut doc, &no_ports).is_err());
        assert_eq!(cells(&doc), vec![(0, 0), (1, 0)], "reverted");
        assert!(e.grab.is_none());
        e.update_grab(&mut doc, Pos2::new(400.0, 0.0));
        assert_eq!(cells(&doc), vec![(0, 0), (1, 0)], "no grab, no move");
    }

    #[test]
    fn duplicate_then_move_or_cancel() {
        let mut doc = layout(&[(0, 0), (1, 0)]);
        let mut e = Editor::new(&doc);
        assert!(!e.duplicate_selected(&mut doc, Pos2::ZERO));
        e.selection.select_all(2);
        assert!(e.duplicate_selected(&mut doc, Pos2::ZERO));
        assert_eq!(doc.components.len(), 4);
        assert_eq!(e.selection.iter().collect::<Vec<_>>(), vec![2, 3]);
        // Confirming in place would overlap the originals.
        assert!(e.confirm_grab(&mut doc, &no_ports).is_err());
        assert_eq!(doc.components.len(), 2);
        assert_eq!(
            e.selection.iter().collect::<Vec<_>>(),
            vec![0, 1],
            "originals re-selected"
        );

        e.selection.select_only(1);
        e.duplicate_selected(&mut doc, Pos2::ZERO);
        e.update_grab(&mut doc, Pos2::new(0.0, 120.0));
        assert!(e.confirm_grab(&mut doc, &no_ports).is_ok());
        assert_eq!(cells(&doc), vec![(0, 0), (1, 0), (1, 3)]);
    }

    #[test]
    fn undo_cancels_grab_and_fixes_selection() {
        let mut doc = layout(&[(0, 0)]);
        let mut e = Editor::new(&doc);
        e.place(&mut doc, DETECTOR, cell_center(4, 4), &no_ports);
        e.history.commit(&doc);
        e.selection.select_only(1);
        e.begin_grab(&doc, Pos2::ZERO, false);
        e.update_grab(&mut doc, Pos2::new(80.0, 0.0));
        assert!(e.undo(&mut doc));
        assert!(e.grab.is_none());
        assert_eq!(cells(&doc), vec![(0, 0)]);
        assert!(e.selection.is_empty(), "index 1 no longer exists");
        assert!(e.redo(&mut doc));
        assert_eq!(cells(&doc), vec![(0, 0), (4, 4)]);
    }
}
