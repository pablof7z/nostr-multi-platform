//! Logical-interest registry — the single writer of the active-interest set (D4).
//!
//! View modules and action modules push `LogicalInterest`s here; the planner
//! reads via [`iter_active`]. The registry is keyed by `InterestId` so that
//! pushing the same interest twice replaces the entry rather than duplicating.
//!
//! Production view modules will withdraw interests when their refcount drops
//! to zero (via the view-warmth grace from `subsystems.md` §7.6). Test code
//! uses [`InterestRegistry::withdraw`] directly.

use std::collections::BTreeMap;

use crate::planner::{InterestId, LogicalInterest};

/// Single-writer registry of active logical interests.
///
/// D4: this is the authoritative active-set; the planner reads via
/// [`iter_active`] but does not mutate. The registry preserves insertion
/// order via a `BTreeMap<InterestId, LogicalInterest>` (sorted by id) so
/// snapshots are deterministic — required for plan-id stability across
/// recompilations.
#[derive(Default)]
pub struct InterestRegistry {
    by_id: BTreeMap<InterestId, LogicalInterest>,
}

impl InterestRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push or replace an interest. Replacing an existing id with a new
    /// shape is the legal way to mutate an interest's filter (the registry
    /// keeps no edit history; the planner sees only the current snapshot).
    pub fn push(&mut self, interest: LogicalInterest) {
        self.by_id.insert(interest.id.clone(), interest);
    }

    /// Withdraw an interest by id. No-op if absent.
    pub fn withdraw(&mut self, id: &InterestId) {
        self.by_id.remove(id);
    }

    /// Snapshot of all active interests, deterministically ordered by id.
    pub fn iter_active(&self) -> Vec<LogicalInterest> {
        self.by_id.values().cloned().collect()
    }

    /// Count of registered interests.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{InterestLifecycle, InterestScope, InterestShape};

    fn fixture(id: u64) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape::default(),
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
        }
    }

    #[test]
    fn push_then_iter_active_returns_inserted() {
        let mut r = InterestRegistry::new();
        r.push(fixture(1));
        r.push(fixture(2));
        let active = r.iter_active();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].id, InterestId(1));
        assert_eq!(active[1].id, InterestId(2));
    }

    #[test]
    fn push_with_same_id_replaces() {
        let mut r = InterestRegistry::new();
        r.push(fixture(1));
        let mut updated = fixture(1);
        updated.lifecycle = InterestLifecycle::OneShot;
        r.push(updated);
        assert_eq!(r.len(), 1);
        assert!(matches!(
            r.iter_active()[0].lifecycle,
            InterestLifecycle::OneShot,
        ));
    }

    #[test]
    fn withdraw_removes() {
        let mut r = InterestRegistry::new();
        r.push(fixture(1));
        r.withdraw(&InterestId(1));
        assert!(r.is_empty());
    }
}
