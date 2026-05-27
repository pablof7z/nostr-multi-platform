//! Ordering helpers for the bounded visible timeline.

use super::super::Kernel;

const TIMELINE_CACHE_LIMIT: usize = 500;

impl Kernel {
    pub(in crate::kernel) fn insert_timeline_id_sorted(&mut self, id: String) {
        if let Some(existing) = self.timeline.iter().position(|current| current == &id) {
            self.timeline.remove(existing);
        }

        let insert_at = self
            .timeline
            .iter()
            .position(|current| self.timeline_id_precedes(&id, current))
            .unwrap_or(self.timeline.len());
        self.timeline.insert(insert_at, id);
        self.timeline.truncate(TIMELINE_CACHE_LIMIT);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::kernel) fn sort_timeline(&mut self) {
        let mut ids = self.timeline.iter().cloned().collect::<Vec<_>>();
        ids.sort_by(|left, right| {
            let a = self.timeline_created_at(left);
            let b = self.timeline_created_at(right);
            b.cmp(&a).then_with(|| left.cmp(right))
        });
        ids.truncate(TIMELINE_CACHE_LIMIT);
        self.timeline = ids.into();
    }

    fn timeline_id_precedes(&self, left: &str, right: &str) -> bool {
        let a = self.timeline_created_at(left);
        let b = self.timeline_created_at(right);
        a > b || (a == b && left < right)
    }

    fn timeline_created_at(&self, id: &str) -> u64 {
        self.events.get(id).map_or(0, |event| event.created_at)
    }
}
