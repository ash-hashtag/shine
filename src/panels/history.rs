//! Snapshot-based undo history, shared by all document editors.

/// Edits are not recorded individually: whenever the document differs from
/// the last committed snapshot and no interaction is in progress, the editor
/// commits, so every kind of change (including slider drags) becomes exactly
/// one undo step.
///
/// Snapshots are whole copies, so the steps kept are bounded by their total
/// weight (see [`Self::weighed`]) as well as their number: a design of a
/// million parts keeps a couple, a small one hundreds.
#[derive(Debug)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    /// The last snapshot; None until the first commit, for a lazy history.
    committed: Option<T>,
    limit: usize,
    /// A snapshot's weight, and the most the undo steps may weigh together.
    weigh: fn(&T) -> usize,
    budget: usize,
    /// Whether the document changed (a commit, undo or redo) since
    /// [`Self::take_changed`] last asked.
    changed: bool,
    /// Commits, undos and redos so far.
    changes: u64,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(document: &T) -> Self {
        Self::with_limit(document, 256)
    }

    pub fn with_limit(document: &T, limit: usize) -> Self {
        Self { committed: Some(document.clone()), ..Self::lazy(limit) }
    }

    /// A history whose first snapshot is taken at the first commit, not
    /// now: a document loaded but never opened isn't copied.
    pub fn lazy(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            committed: None,
            limit,
            weigh: |_| 1,
            budget: usize::MAX,
            changed: false,
            changes: 0,
        }
    }

    /// Bound the undo steps by their total weight as `weigh` measures them,
    /// keeping at least one.
    pub fn weighed(self, weigh: fn(&T) -> usize, budget: usize) -> Self {
        Self { weigh, budget, ..self }
    }

    /// For a lazy history, take the first snapshot now if not yet taken:
    /// when the document is about to be edited.
    pub fn start(&mut self, current: &T) {
        if self.committed.is_none() {
            self.committed = Some(current.clone());
        }
    }

    /// Commits, undos and redos so far: changes when the document did.
    pub fn changes(&self) -> u64 {
        self.changes
    }

    /// Whether the document changed since this was last asked.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Record `current` if it changed since the last commit.
    pub fn commit(&mut self, current: &T) -> bool {
        let Some(committed) = &mut self.committed else {
            self.committed = Some(current.clone());
            return false;
        };
        if current == committed {
            return false;
        }
        let previous = std::mem::replace(committed, current.clone());
        self.undo.push(previous);
        let weigh = self.weigh;
        while self.undo.len() > self.limit
            || (self.undo.len() > 1 && self.undo.iter().map(weigh).fold(0usize, usize::saturating_add) > self.budget)
        {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.changed = true;
        self.changes += 1;
        true
    }

    pub fn undo(&mut self, current: &mut T) -> bool {
        self.commit(current);
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(current, previous));
        self.committed = Some(current.clone());
        self.changed = true;
        self.changes += 1;
        true
    }

    pub fn redo(&mut self, current: &mut T) -> bool {
        self.commit(current);
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(current, next));
        self.committed = Some(current.clone());
        self.changed = true;
        self.changes += 1;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo() {
        let mut doc = vec![0];
        let mut h = History::new(&doc);
        assert!(!h.can_undo() && !h.can_redo());
        assert!(!h.commit(&doc), "no change, no step");

        doc[0] = 1;
        assert!(h.commit(&doc));
        doc.clear();
        // Uncommitted edits are committed by undo, then undone.
        assert!(h.undo(&mut doc));
        assert_eq!(doc, vec![1]);
        assert!(h.undo(&mut doc));
        assert_eq!(doc, vec![0]);
        assert!(!h.undo(&mut doc));
        assert!(h.can_redo());
        assert!(h.redo(&mut doc));
        assert!(h.redo(&mut doc));
        assert!(doc.is_empty());
        assert!(!h.redo(&mut doc));

        // A new edit clears the redo stack.
        h.undo(&mut doc);
        doc.push(5);
        h.commit(&doc);
        assert!(!h.can_redo());
    }

    #[test]
    fn bounded() {
        let mut doc = 0;
        let mut h = History::with_limit(&doc, 3);
        for k in 1..=10 {
            doc = k;
            h.commit(&doc);
        }
        let mut steps = 0;
        while h.undo(&mut doc) {
            steps += 1;
        }
        assert_eq!((steps, doc), (3, 7));
    }

    #[test]
    fn lazy_and_weighed() {
        // The first commit is the starting point, not a step.
        let mut doc = vec![1, 2];
        let mut h = History::lazy(256).weighed(Vec::len, 10);
        assert!(!h.commit(&doc) && !h.can_undo());
        // Steps of 4 items each: two fit in 10, then the oldest go.
        for k in 0..5 {
            doc = vec![k; 4];
            h.commit(&doc);
        }
        let mut steps = 0;
        while h.undo(&mut doc) {
            steps += 1;
        }
        assert_eq!((steps, doc), (2, vec![2; 4]));
    }
}
