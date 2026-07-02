use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub trait FeedController: Send + Sync {
    fn load_older(&self) -> bool;

    /// Signal a perspective change. Controller implementations should coordinate
    /// visible reset and cursor rewind under the serialized host contract,
    /// returning whether visible state actually changed (so the host can decide
    /// whether to re-emit a snapshot).
    ///
    /// Default: a no-op returning `false`.
    fn reset(&self) -> bool {
        false
    }

    /// Evict a single source from the feed's visible state by its lowercase-hex
    /// event id, returning whether the feed accepted the request. Used to
    /// remove a superseded replaceable event. Default: a no-op returning
    /// `false`, for controllers with no replacement hook.
    fn replace_source(&self, _source_id: &str) -> bool {
        false
    }
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
        self.with_controller(key, |controller| controller.load_older())
    }

    /// Reset the feed registered under `key` for a perspective change, returning
    /// whether visible state changed. An absent key or a poisoned lock fails
    /// closed (returns `false`, no panic) — a perspective change for a feed that
    /// is not registered is a silent no-op (D6: teardown/reset is best-effort).
    #[must_use]
    pub fn reset(&self, key: &str) -> bool {
        self.with_controller(key, |controller| controller.reset())
    }

    /// Evict a source from the feed registered under `key` by its hex event id,
    /// returning whether the feed accepted the request. Absent key / poisoned
    /// lock fail closed.
    #[must_use]
    pub fn replace(&self, key: &str, source_id: &str) -> bool {
        self.with_controller(key, |controller| controller.replace_source(source_id))
    }

    /// Resolve the controller under `key` and run `f`, cloning the `Arc` out of
    /// the lock first so the per-feed operation never runs while the registry
    /// map is held. Absent key or poisoned lock ⇒ `false` (fail closed).
    fn with_controller(&self, key: &str, f: impl FnOnce(&dyn FeedController) -> bool) -> bool {
        let controller = self
            .feeds
            .lock()
            .ok()
            .and_then(|feeds| feeds.get(key).cloned());
        controller.is_some_and(|controller| f(controller.as_ref()))
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

    // `FeedRegistry::register`/`unregister` key on a bare `&str` with no
    // ownership check (unlike `register_typed_snapshot_projection`'s
    // `Into<ProjectionRegistrationKey>` bound, PR #2610) — these are arbitrary
    // test keys for the raw internal registry, not an app-facing naming
    // example.

    struct StubController(bool);
    impl FeedController for StubController {
        fn load_older(&self) -> bool {
            self.0
        }
    }

    /// Records every reset / replace_source call so registry passthrough is
    /// observable, and reports a fixed boolean for each.
    #[derive(Default)]
    struct PerspectiveController {
        resets: Mutex<usize>,
        replaced: Mutex<Vec<String>>,
        reset_changed: bool,
        replace_accepted: bool,
    }
    impl FeedController for PerspectiveController {
        fn load_older(&self) -> bool {
            false
        }
        fn reset(&self) -> bool {
            *self.resets.lock().unwrap() += 1;
            self.reset_changed
        }
        fn replace_source(&self, source_id: &str) -> bool {
            self.replaced.lock().unwrap().push(source_id.to_string());
            self.replace_accepted
        }
    }

    #[test]
    fn register_then_unregister_removes_the_controller() {
        let reg = FeedRegistry::default();
        reg.register("test.feed.author.alice", Arc::new(StubController(true)));
        // Present: load_older reaches the controller.
        assert!(reg.load_older("test.feed.author.alice"));
        // Removed: returns true once, then false (idempotent), and the key
        // no longer resolves a controller.
        assert!(reg.unregister("test.feed.author.alice"));
        assert!(!reg.unregister("test.feed.author.alice"));
        assert!(!reg.load_older("test.feed.author.alice"));
    }

    #[test]
    fn unregister_absent_key_is_a_noop() {
        let reg = FeedRegistry::default();
        assert!(!reg.unregister("test.feed.thread.missing"));
    }

    #[test]
    fn reset_and_replace_pass_through_to_the_controller_by_key() {
        let reg = FeedRegistry::default();
        let ctrl = Arc::new(PerspectiveController {
            reset_changed: true,
            replace_accepted: true,
            ..Default::default()
        });
        reg.register("test.feed.author.alice", ctrl.clone());

        assert!(
            reg.reset("test.feed.author.alice"),
            "reset reached the feed"
        );
        assert!(
            reg.replace("test.feed.author.alice", "deadbeef"),
            "replace reached the feed"
        );
        assert_eq!(
            *ctrl.resets.lock().unwrap(),
            1,
            "reset invoked exactly once"
        );
        assert_eq!(
            ctrl.replaced.lock().unwrap().as_slice(),
            &["deadbeef".to_string()],
            "the source id was forwarded verbatim"
        );
    }

    #[test]
    fn reset_and_replace_on_absent_key_fail_closed() {
        let reg = FeedRegistry::default();
        assert!(
            !reg.reset("test.feed.missing"),
            "absent key ⇒ reset is false"
        );
        assert!(
            !reg.replace("test.feed.missing", "id"),
            "absent key ⇒ replace is false"
        );
    }

    #[test]
    fn passthrough_fails_closed_on_a_poisoned_lock() {
        let reg = Arc::new(FeedRegistry::default());
        reg.register("test.feed.author.alice", Arc::new(StubController(true)));
        // Poison the feeds map by panicking while holding its lock.
        let poisoner = Arc::clone(&reg);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.feeds.lock().unwrap();
            panic!("poison the registry");
        })
        .join();

        assert!(
            !reg.load_older("test.feed.author.alice"),
            "poisoned ⇒ false"
        );
        assert!(!reg.reset("test.feed.author.alice"), "poisoned ⇒ false");
        assert!(
            !reg.replace("test.feed.author.alice", "id"),
            "poisoned ⇒ false"
        );
        assert!(
            !reg.unregister("test.feed.author.alice"),
            "poisoned ⇒ false"
        );
    }
}
