//! Snapshot-projection teardown helpers for reducer-backed sessions.

use std::sync::Arc;

impl super::KernelReducer {
    /// Remove a feed's typed snapshot projection and paired author provider.
    ///
    /// Session teardown uses this for dynamic feed projections whose typed
    /// sidecar and feed-author provider share the same key. The typed removal
    /// emits the registry's one-shot `Cleared` row; the provider removal stops
    /// future auto-resolve contributions, letting the kernel reconcile release
    /// refs on the next snapshot tick.
    pub fn remove_feed_snapshot_projection(&self, feed_key: &str) {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.remove(feed_key);
            guard.remove_feed_author_provider(feed_key);
        }
    }

    /// Build a `Send` teardown action for [`Self::remove_feed_snapshot_projection`].
    ///
    /// Feed sessions store teardown as `Box<dyn FnOnce() + Send>`, so callers
    /// must capture only the snapshot registry slot, not the full reducer.
    pub fn remove_feed_snapshot_projection_action(
        &self,
        feed_key: impl Into<String>,
    ) -> Box<dyn FnOnce() + Send> {
        let slot = Arc::clone(&self.snapshot_slot);
        let feed_key = feed_key.into();
        Box::new(move || {
            if let Ok(mut guard) = slot.lock() {
                guard.remove(&feed_key);
                guard.remove_feed_author_provider(&feed_key);
            }
        })
    }

    /// Build a `Send` teardown action that removes one typed projection.
    pub fn remove_snapshot_projection_action(
        &self,
        key: impl Into<String>,
    ) -> Box<dyn FnOnce() + Send> {
        let slot = Arc::clone(&self.snapshot_slot);
        let key = key.into();
        Box::new(move || {
            if let Ok(mut guard) = slot.lock() {
                guard.remove(&key);
            }
        })
    }
}
