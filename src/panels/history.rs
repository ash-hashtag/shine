//! Snapshot-based undo history, shared by all document editors.

/// Edits are not recorded individually: whenever the document differs from
/// the last committed snapshot and no interaction is in progress, the editor
/// commits, so every kind of change (including slider drags) becomes exactly
/// one undo step.
#[derive(Debug)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    committed: T,
    limit: usize,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(document: &T) -> Self {
        Self::with_limit(document, 256)
    }

    pub fn with_limit(document: &T, limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            committed: document.clone(),
            limit,
        }
    }

    /// Record `current` if it changed since the last commit.
    pub fn commit(&mut self, current: &T) -> bool {
        if *current == self.committed {
            return false;
        }
        let previous = std::mem::replace(&mut self.committed, current.clone());
        self.undo.push(previous);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    pub fn undo(&mut self, current: &mut T) -> bool {
        self.commit(current);
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(current, previous));
        self.committed = current.clone();
        true
    }

    pub fn redo(&mut self, current: &mut T) -> bool {
        self.commit(current);
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(current, next));
        self.committed = current.clone();
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
}
