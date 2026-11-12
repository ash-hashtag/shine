//! Where a layout's boxes and waveguides are: a grid of buckets over the
//! world, so the canvas draws and hit-tests only what is in view or under
//! the pointer. On a layout of hundreds of thousands of boxes a frame then
//! costs what the screen shows, not what the layout holds.

use std::collections::HashMap;

use egui::{Pos2, Rect};

use super::model::GRID;

/// World units per bucket side: 16 grid cells.
const BUCKET: f32 = 16.0 * GRID;

/// An item covering more buckets than this is listed once, as everywhere.
const MAX_BUCKETS: i64 = 256;

/// What decides the index: when any of it changes, it's built again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexKey {
    /// Changes to the layout recorded by its history (edits, undo, redo).
    pub changes: u64,
    pub instances: usize,
    pub waveguides: usize,
    /// The port counts of the components it uses (they size the boxes).
    pub interfaces: u64,
}

/// Items by the buckets their bounds touch.
#[derive(Debug, Default)]
struct Buckets {
    map: HashMap<(i32, i32), Vec<u32>>,
    everywhere: Vec<u32>,
}

fn bucket_of(p: Pos2) -> (i32, i32) {
    ((p.x / BUCKET).floor() as i32, (p.y / BUCKET).floor() as i32)
}

impl Buckets {
    fn insert(&mut self, item: usize, bounds: Rect) {
        let (lo, hi) = (bucket_of(bounds.min), bucket_of(bounds.max));
        let count = (hi.0 as i64 - lo.0 as i64 + 1) * (hi.1 as i64 - lo.1 as i64 + 1);
        if count > MAX_BUCKETS {
            self.everywhere.push(item as u32);
            return;
        }
        for bx in lo.0..=hi.0 {
            for by in lo.1..=hi.1 {
                self.map.entry((bx, by)).or_default().push(item as u32);
            }
        }
    }

    /// Items whose bounds may touch `area`, ascending, each once.
    fn query(&self, area: Rect) -> Vec<usize> {
        let (lo, hi) = (bucket_of(area.min), bucket_of(area.max));
        let mut out: Vec<usize> = self.everywhere.iter().map(|&i| i as usize).collect();
        let buckets = (hi.0 as i64 - lo.0 as i64 + 1) * (hi.1 as i64 - lo.1 as i64 + 1);
        if buckets > self.map.len() as i64 {
            // Zoomed far out: going through the buckets there are is quicker.
            for (b, items) in &self.map {
                if (lo.0..=hi.0).contains(&b.0) && (lo.1..=hi.1).contains(&b.1) {
                    out.extend(items.iter().map(|&i| i as usize));
                }
            }
        } else {
            for bx in lo.0..=hi.0 {
                for by in lo.1..=hi.1 {
                    if let Some(items) = self.map.get(&(bx, by)) {
                        out.extend(items.iter().map(|&i| i as usize));
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// The boxes and waveguides of a layout by where they are.
#[derive(Debug)]
pub struct SpatialIndex {
    pub key: IndexKey,
    boxes: Buckets,
    waveguides: Buckets,
}

impl SpatialIndex {
    /// From each box's bounds (by index in the layout's instances) and each
    /// waveguide's (numbered as [`super::OpticsPanel::waveguide_count`]).
    pub fn new(key: IndexKey, boxes: impl Iterator<Item = Rect>, waveguides: impl Iterator<Item = Option<Rect>>) -> Self {
        let mut index = Self { key, boxes: Buckets::default(), waveguides: Buckets::default() };
        for (b, r) in boxes.enumerate() {
            index.boxes.insert(b, r);
        }
        for (w, r) in waveguides.enumerate() {
            match r {
                Some(r) => index.waveguides.insert(w, r),
                // Its ends unknown: drawn wherever it is.
                None => index.waveguides.everywhere.push(w as u32),
            }
        }
        index
    }

    /// Boxes (indices into the instances) that may touch `area`, in order.
    pub fn boxes_in(&self, area: Rect) -> Vec<usize> {
        self.boxes.query(area)
    }

    /// Waveguides that may pass through `area`, in order.
    pub fn waveguides_in(&self, area: Rect) -> Vec<usize> {
        self.waveguides.query(area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_what_touches_an_area_and_only_that() {
        let key = IndexKey { changes: 0, instances: 3, waveguides: 2, interfaces: 0 };
        let at = |x: f32, y: f32| Rect::from_min_size(Pos2::new(x, y), egui::Vec2::splat(GRID));
        let far = 1000.0 * BUCKET;
        let index = SpatialIndex::new(
            key,
            [at(0.0, 0.0), at(far, far), at(-far, 0.0)].into_iter(),
            [Some(Rect::from_two_pos(Pos2::new(0.0, 0.0), Pos2::new(far, 0.0))), None].into_iter(),
        );
        let near_origin = Rect::from_center_size(Pos2::ZERO, egui::Vec2::splat(4.0 * GRID));
        assert_eq!(index.boxes_in(near_origin), vec![0]);
        assert_eq!(index.boxes_in(at(far, far)), vec![1]);
        // A long waveguide is everywhere; one with unknown ends is too.
        assert_eq!(index.waveguides_in(at(-far, far)), vec![0, 1]);
        // Zoomed out over everything: all of it, each once.
        let all = Rect::from_min_max(Pos2::new(-2.0 * far, -2.0 * far), Pos2::new(2.0 * far, 2.0 * far));
        assert_eq!(index.boxes_in(all), vec![0, 1, 2]);
    }
}
