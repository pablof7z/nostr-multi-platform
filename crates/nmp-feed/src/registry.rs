use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub trait FeedController: Send + Sync {
    fn load_older(&self) -> bool;
}

#[derive(Default)]
pub struct FeedRegistry {
    feeds: Mutex<BTreeMap<String, Arc<dyn FeedController>>>,
}

impl FeedRegistry {
    pub fn register(&self, key: impl Into<String>, controller: Arc<dyn FeedController>) {
        if let Ok(mut feeds) = self.feeds.lock() {
            feeds.insert(key.into(), controller);
        }
    }

    /// Drop the controller registered under `key`.
    ///
    /// Used by transient feeds (a visited profile / open thread) whose
    /// snapshot key must not outlive the screen. Absent keys are a no-op;
    /// a poisoned lock is a silent no-op (D6: teardown is best-effort, the
    /// `nmp_app_free` actor join is the hard fence). Returns `true` when a
    /// controller was actually removed.
    pub fn unregister(&self, key: &str) -> bool {
        self.feeds
            .lock()
            .ok()
            .and_then(|mut feeds| feeds.remove(key))
            .is_some()
    }

    #[must_use]
    pub fn load_older(&self, key: &str) -> bool {
        let controller = self
            .feeds
            .lock()
            .ok()
            .and_then(|feeds| feeds.get(key).cloned());
        controller.is_some_and(|controller| controller.load_older())
    }
}

pub type FeedRegistrySlot = Arc<FeedRegistry>;

#[must_use]
pub fn new_feed_registry_slot() -> FeedRegistrySlot {
    Arc::new(FeedRegistry::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubController(bool);
    impl FeedController for StubController {
        fn load_older(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn register_then_unregister_removes_the_controller() {
        let reg = FeedRegistry::default();
        reg.register("nmp.feed.author.alice", Arc::new(StubController(true)));
        // Present: load_older reaches the controller.
        assert!(reg.load_older("nmp.feed.author.alice"));
        // Removed: returns true once, then false (idempotent), and the key
        // no longer resolves a controller.
        assert!(reg.unregister("nmp.feed.author.alice"));
        assert!(!reg.unregister("nmp.feed.author.alice"));
        assert!(!reg.load_older("nmp.feed.author.alice"));
    }

    #[test]
    fn unregister_absent_key_is_a_noop() {
        let reg = FeedRegistry::default();
        assert!(!reg.unregister("nmp.feed.thread.missing"));
    }
}
