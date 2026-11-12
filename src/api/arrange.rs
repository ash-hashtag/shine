//! Positions for parts added without one, so a design made in code reads
//! well in the app: left to right in the order light and signals flow,
//! each column stacked in the order the parts were added (so pins keep
//! their order), to the right of anything already placed.

use std::collections::{HashMap, HashSet};

use super::project::Inner;
use crate::panels::optics::model::{BlockRef, Builtin, Item, OpticComponentKind, box_size, cell_center, cell_of};
use crate::panels::project::DocId;

/// Room between columns, and between parts in a column, in cells.
const COLUMN_GAP: i32 = 4;
const ROW_GAP: i32 = 1;

pub(crate) fn arrange_all(inner: &mut Inner) {
    let docs: HashSet<DocId> = inner.unplaced.iter().map(|(d, _)| *d).collect();
    for doc in docs {
        arrange(inner, doc);
    }
    inner.unplaced.clear();
}

fn arrange(inner: &mut Inner, doc: DocId) {
    let catalog = inner.project.catalog();
    let unplaced: HashSet<Item> = inner.unplaced.iter().filter(|(d, _)| *d == doc).map(|(_, i)| *i).collect();
    let Some(d) = inner.project.doc_mut(doc) else { return };
    let layout = d.layout_mut();
    let size = |item: Item| -> (i32, i32) {
        match item {
            Item::Box(id) => layout.instance(id).map_or((2, 1), |i| box_size(catalog.counts(&i.block))),
            Item::Part(_) => (1, 1),
        }
    };
    // In the order added: boxes, then signal pins.
    let items: Vec<Item> = layout
        .instances
        .iter()
        .map(|i| Item::Box(i.id))
        .chain((0..layout.components.len()).map(Item::Part))
        .filter(|i| unplaced.contains(i))
        .collect();
    if items.is_empty() {
        return;
    }
    // Start right of whatever is already placed.
    let right = layout
        .instances
        .iter()
        .filter(|i| !unplaced.contains(&Item::Box(i.id)))
        .map(|i| i.cell.0 + box_size(catalog.counts(&i.block)).0)
        .chain(layout.components.iter().enumerate().filter(|(k, _)| !unplaced.contains(&Item::Part(*k))).map(|(_, c)| cell_of(c.pos).0 + 1))
        .max();
    let base_x = right.map_or(0, |r| r + COLUMN_GAP);

    // Each part's column: one past the furthest part feeding it.
    let edges: Vec<(Item, Item)> = layout
        .wires
        .iter()
        .map(|w| (Item::Box(w.from.instance), Item::Box(w.to.instance)))
        .chain(layout.signals.iter().map(|s| (s.from.item, s.to.item)))
        .filter(|(a, b)| unplaced.contains(a) && unplaced.contains(b))
        .collect();
    let mut depth: HashMap<Item, i32> = items.iter().map(|&i| (i, 0)).collect();
    for _ in 0..items.len() {
        let mut changed = false;
        for &(a, b) in &edges {
            let next = depth[&a] + 1;
            if next > depth[&b] && next <= items.len() as i32 {
                depth.insert(b, next);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // A design's outputs go last.
    let last = depth.values().copied().max().unwrap_or(0) + 1;
    for &item in &items {
        let is_output = match item {
            Item::Box(id) => matches!(layout.instance(id).map(|i| &i.block), Some(BlockRef::Builtin(Builtin::OutputPin { .. }))),
            Item::Part(k) => matches!(layout.components.get(k).map(|c| c.kind), Some(OpticComponentKind::SignalOut)),
        };
        if is_output {
            depth.insert(item, last);
        }
    }

    // Columns as wide as their widest part.
    let columns = depth.values().copied().max().unwrap_or(0) + 1;
    let mut widths = vec![1; columns as usize];
    for &item in &items {
        let w = &mut widths[depth[&item] as usize];
        *w = (*w).max(size(item).0);
    }
    let mut x = base_x;
    let mut column_x = Vec::new();
    for w in &widths {
        column_x.push(x);
        x += w + COLUMN_GAP;
    }
    let mut row = vec![0; columns as usize];
    let mut cells: Vec<(Item, (i32, i32))> = Vec::new();
    for &item in &items {
        let c = depth[&item] as usize;
        cells.push((item, (column_x[c], row[c])));
        row[c] += size(item).1 + ROW_GAP;
    }
    for (item, (x, y)) in cells {
        match item {
            Item::Box(id) => {
                if let Some(i) = layout.instances.iter_mut().find(|i| i.id == id) {
                    i.cell = (x, y);
                }
            }
            Item::Part(k) => {
                if let Some(c) = layout.components.get_mut(k) {
                    c.pos = cell_center(x, y);
                }
            }
        }
    }
}
