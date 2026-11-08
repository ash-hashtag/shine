//! Index-based selection with an active item, shared by document editors.

use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub struct Selection {
    items: BTreeSet<usize>,
    active: Option<usize>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn contains(&self, i: usize) -> bool {
        self.items.contains(&i)
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.items.iter().copied()
    }

    /// The most recently selected item, whose properties are shown.
    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.active = None;
    }

    pub fn select_only(&mut self, i: usize) {
        self.items.clear();
        self.add(i);
    }

    pub fn add(&mut self, i: usize) {
        self.items.insert(i);
        self.active = Some(i);
    }

    /// Blender's Shift+click: select, or deselect if already active.
    pub fn toggle(&mut self, i: usize) {
        if self.active == Some(i) {
            self.items.remove(&i);
            self.active = self.items.iter().next_back().copied();
        } else {
            self.add(i);
        }
    }

    pub fn select_all(&mut self, count: usize) {
        self.items = (0..count).collect();
        if self.active.is_none_or(|a| a >= count) {
            self.active = count.checked_sub(1);
        }
    }

    pub fn invert(&mut self, count: usize) {
        self.items = (0..count).filter(|i| !self.items.contains(i)).collect();
        if !self.active.is_some_and(|a| self.items.contains(&a)) {
            self.active = self.items.iter().next_back().copied();
        }
    }

    /// Drop indices that no longer exist.
    pub fn retain_valid(&mut self, count: usize) {
        self.items.retain(|&i| i < count);
        if self.active.is_some_and(|a| a >= count) {
            self.active = self.items.iter().next_back().copied();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_semantics() {
        let mut s = Selection::default();
        assert!(s.is_empty() && s.active().is_none());
        s.select_only(2);
        s.add(4);
        assert_eq!((s.len(), s.active()), (2, Some(4)));
        // Shift-click on a non-active item makes it active.
        s.toggle(2);
        assert_eq!((s.contains(2), s.active()), (true, Some(2)));
        // Shift-click on the active item deselects it.
        s.toggle(2);
        assert_eq!((s.contains(2), s.active()), (false, Some(4)));
        s.select_all(5);
        assert_eq!(s.len(), 5);
        assert_eq!(s.active(), Some(4));
        s.select_only(1);
        s.invert(4);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![0, 2, 3]);
        assert_eq!(s.active(), Some(3));
        s.retain_valid(3);
        assert_eq!(
            (s.iter().collect::<Vec<_>>(), s.active()),
            (vec![0, 2], Some(2))
        );
        s.clear();
        assert!(s.is_empty());
        s.select_all(0);
        assert_eq!(s.active(), None);
    }
}
