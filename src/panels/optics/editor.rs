//! Editing operations on a [`Layout`], independent of any UI.

use egui::{Pos2, Rect, Vec2};

use super::model::{GRID, Layout, OpticComponentKind, PlacedComponent, cell_of, snap};
pub use crate::panels::history::History;
pub use crate::panels::selection::Selection;

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
    pub originals: Vec<(usize, Pos2)>,
    pub axis: Axis,
    /// Started by Shift+D: the components that were copied. Cancelling
    /// removes the copies and re-selects these.
    pub duplicated_from: Option<Vec<usize>>,
    /// Started by dragging with the mouse: releasing the button confirms.
    pub tweak: bool,
    /// Whole cells moved so far.
    pub offset: (i32, i32),
}

#[derive(Debug)]
pub struct Editor {
    pub selection: Selection,
    pub history: History<Layout>,
    pub grab: Option<Grab>,
}

impl Editor {
    pub fn new(layout: &Layout) -> Self {
        Self {
            selection: Selection::default(),
            history: History::new(layout),
            grab: None,
        }
    }

    /// Left click at `world`. Returns whether a component was hit.
    pub fn click_select(&mut self, layout: &Layout, world: Pos2, extend: bool) -> bool {
        match (layout.component_at(world), extend) {
            (Some(i), false) => self.selection.select_only(i),
            (Some(i), true) => self.selection.toggle(i),
            (None, false) => self.selection.clear(),
            (None, true) => {}
        }
        layout.component_at(world).is_some()
    }

    /// Select components whose centres fall inside `area` (world space).
    pub fn box_select(&mut self, layout: &Layout, area: Rect, extend: bool) {
        if !extend {
            self.selection.clear();
        }
        for (i, c) in layout.components.iter().enumerate() {
            if area.contains(c.pos) {
                self.selection.add(i);
            }
        }
    }

    /// Place a new component in the cell at `world` unless it is occupied.
    pub fn place(
        &mut self,
        layout: &mut Layout,
        kind: OpticComponentKind,
        world: Pos2,
    ) -> Option<usize> {
        if layout.component_at(world).is_some() {
            return None;
        }
        layout.components.push(PlacedComponent {
            kind,
            pos: snap(world),
        });
        let i = layout.components.len() - 1;
        self.selection.select_only(i);
        Some(i)
    }

    pub fn delete_selected(&mut self, layout: &mut Layout) -> usize {
        let doomed: Vec<usize> = self.selection.iter().collect();
        for &i in doomed.iter().rev() {
            layout.components.remove(i);
        }
        self.selection.clear();
        doomed.len()
    }

    pub fn rotate_selected(&mut self, layout: &mut Layout, steps: i32) {
        for i in self.selection.iter() {
            layout.components[i].kind.rotate(steps);
        }
    }

    /// Start moving the selection. Returns false when nothing is selected.
    pub fn begin_grab(&mut self, layout: &Layout, mouse: Pos2, tweak: bool) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        self.grab = Some(Grab {
            start: mouse,
            originals: self
                .selection
                .iter()
                .map(|i| (i, layout.components[i].pos))
                .collect(),
            axis: Axis::Free,
            duplicated_from: None,
            tweak,
            offset: (0, 0),
        });
        true
    }

    /// Copy the selection and start moving the copies.
    pub fn duplicate_selected(&mut self, layout: &mut Layout, mouse: Pos2) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let sources: Vec<usize> = self.selection.iter().collect();
        let copies: Vec<PlacedComponent> = sources.iter().map(|&i| layout.components[i]).collect();
        let first = layout.components.len();
        layout.components.extend(copies);
        self.selection.clear();
        for i in first..layout.components.len() {
            self.selection.add(i);
        }
        self.begin_grab(layout, mouse, false);
        if let Some(g) = &mut self.grab {
            g.duplicated_from = Some(sources);
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
        let delta = mouse - g.start;
        let mut cells = (
            (delta.x / GRID).round() as i32,
            (delta.y / GRID).round() as i32,
        );
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
    }

    /// Finish the move. Fails (and reverts) if components would overlap.
    pub fn confirm_grab(&mut self, layout: &mut Layout) -> Result<(), &'static str> {
        if self.grab.is_none() {
            return Ok(());
        }
        let mut cells = std::collections::HashSet::new();
        let overlapping = layout
            .components
            .iter()
            .any(|c| !cells.insert(cell_of(c.pos)));
        if overlapping {
            self.cancel_grab(layout);
            return Err("cells already occupied");
        }
        self.grab = None;
        Ok(())
    }

    pub fn cancel_grab(&mut self, layout: &mut Layout) {
        let Some(g) = self.grab.take() else { return };
        if let Some(sources) = g.duplicated_from {
            let first = g
                .originals
                .iter()
                .map(|o| o.0)
                .min()
                .unwrap_or(layout.components.len());
            layout.components.truncate(first);
            self.selection.clear();
            for i in sources {
                self.selection.add(i);
            }
        } else {
            for (i, original) in g.originals {
                layout.components[i].pos = original;
            }
        }
    }

    pub fn undo(&mut self, layout: &mut Layout) -> bool {
        self.cancel_grab(layout);
        let changed = self.history.undo(layout);
        self.selection.retain_valid(layout.components.len());
        changed
    }

    pub fn redo(&mut self, layout: &mut Layout) -> bool {
        self.cancel_grab(layout);
        let changed = self.history.redo(layout);
        self.selection.retain_valid(layout.components.len());
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::optics::model::cell_center;

    fn layout(cells: &[(i32, i32)]) -> Layout {
        Layout {
            components: cells
                .iter()
                .map(|&(i, j)| PlacedComponent {
                    kind: OpticComponentKind::Lens,
                    pos: cell_center(i, j),
                })
                .collect(),
            lines: Vec::new(),
        }
    }

    fn cells(layout: &Layout) -> Vec<(i32, i32)> {
        layout.components.iter().map(|c| cell_of(c.pos)).collect()
    }

    #[test]
    fn click_and_box_select() {
        let doc = layout(&[(0, 0), (1, 0), (5, 5)]);
        let mut e = Editor::new(&doc);
        assert!(e.click_select(&doc, cell_center(1, 0), false));
        assert_eq!(e.selection.iter().collect::<Vec<_>>(), vec![1]);
        assert!(e.click_select(&doc, cell_center(0, 0), true));
        assert_eq!(e.selection.len(), 2);
        // Shift-click on empty space keeps the selection; plain click clears it.
        assert!(!e.click_select(&doc, cell_center(9, 9), true));
        assert_eq!(e.selection.len(), 2);
        e.click_select(&doc, cell_center(9, 9), false);
        assert!(e.selection.is_empty());

        let area = Rect::from_min_max(Pos2::new(-10.0, -10.0), Pos2::new(70.0, 30.0));
        e.box_select(&doc, area, false);
        assert_eq!(e.selection.iter().collect::<Vec<_>>(), vec![0, 1]);
        let far = Rect::from_center_size(cell_center(5, 5), Vec2::splat(10.0));
        e.box_select(&doc, far, true);
        assert_eq!(e.selection.len(), 3);
        e.box_select(&doc, far, false);
        assert_eq!(e.selection.iter().collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn place_rejects_occupied_cells() {
        let mut doc = layout(&[(0, 0)]);
        let mut e = Editor::new(&doc);
        assert_eq!(
            e.place(&mut doc, OpticComponentKind::Detector, cell_center(0, 0)),
            None
        );
        let i = e.place(&mut doc, OpticComponentKind::Detector, Pos2::new(45.0, 3.0));
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
        assert!(e.confirm_grab(&mut doc).is_ok());
        assert!(e.grab.is_none());
        assert_eq!(cells(&doc), vec![(0, 2), (1, 0)]);
        assert!(e.confirm_grab(&mut doc).is_ok(), "no grab is a no-op");
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
        assert!(e.confirm_grab(&mut doc).is_err());
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
        assert!(e.confirm_grab(&mut doc).is_err());
        assert_eq!(doc.components.len(), 2);
        assert_eq!(
            e.selection.iter().collect::<Vec<_>>(),
            vec![0, 1],
            "originals re-selected"
        );

        e.selection.select_only(1);
        e.duplicate_selected(&mut doc, Pos2::ZERO);
        e.update_grab(&mut doc, Pos2::new(0.0, 120.0));
        assert!(e.confirm_grab(&mut doc).is_ok());
        assert_eq!(cells(&doc), vec![(0, 0), (1, 0), (1, 3)]);
    }

    #[test]
    fn undo_cancels_grab_and_fixes_selection() {
        let mut doc = layout(&[(0, 0)]);
        let mut e = Editor::new(&doc);
        e.place(&mut doc, OpticComponentKind::Detector, cell_center(4, 4));
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
