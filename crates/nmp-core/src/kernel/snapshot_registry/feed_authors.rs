//! ADR-0070 D7 (#1671 Lane H) — the feed-author auto-resolve provider registry
//! and the emitted-author sink for the structural guardrail.
//!
//! Extracted from `snapshot_registry.rs` (the `impl SnapshotRegistry` methods
//! that read/write the `feed_author_providers` / `emitted_feed_authors` fields)
//! so that file stays under the 500-LOC hard ceiling — the same submodule
//! pattern `kernel_access.rs` / `incremental_apply.rs` already use. The two
//! backing fields remain on the `SnapshotRegistry` struct in the parent module.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use super::bounds::admit_keyed;
use super::{record_emitted_feed_authors, EmittedFeedAuthorsSlot, SnapshotRegistry};

impl SnapshotRegistry {
    /// ADR-0070 D7 (#1671 Lane H) — register the feed-author-set provider for
    /// `feed_key` (e.g. `"microblog.timeline.home"`).
    ///
    /// Last-writer-wins on the key (a re-register replaces the closure, no
    /// duplicate). Bounded by the same `MAX_SNAPSHOT_PROJECTIONS` ceiling as the
    /// typed registry (one provider per feed) — at the cap a NEW key is a loud
    /// no-op (D5/D6: `tracing::warn!`, no panic); replacing an existing key is
    /// always allowed.
    pub fn register_feed_author_provider(
        &mut self,
        feed_key: impl Into<String>,
        f: impl Fn() -> Vec<String> + Send + Sync + 'static,
    ) {
        let feed_key = feed_key.into();
        let exists = self.feed_author_providers.contains_key(&feed_key);
        if !admit_keyed(
            self.feed_author_providers.len(),
            exists,
            &feed_key,
            "feed-author provider",
        ) {
            return;
        }
        self.feed_author_providers.insert(feed_key, Box::new(f));
    }

    /// Return the set of feed-author-provider keys currently registered —
    /// without running any provider closure.
    ///
    /// Intended for structural-pairing coverage tests (ADR-0070 D7, #1671 Lane H):
    /// a test asserts that registering a feed's typed sidecar ALSO registered its
    /// author provider under the same key (and that closing the feed removes
    /// both), proving the pairing cannot be split.
    #[must_use]
    pub fn registered_feed_author_provider_keys(&self) -> impl Iterator<Item = &str> {
        self.feed_author_providers.keys().map(|k| k.as_str())
    }

    /// Remove the feed-author provider registered under `feed_key`.
    ///
    /// Returns `true` when one was present. Called from `unregister_feed` (a
    /// transient author/thread feed closing) so its provider stops contributing
    /// and the kernel's next reconcile releases every ref the provider claimed.
    pub fn remove_feed_author_provider(&mut self, feed_key: &str) -> bool {
        self.feed_author_providers.remove(feed_key).is_some()
    }

    /// ADR-0070 D7 (#1671 Lane H) — record the actual author keys a feed's typed
    /// producer ENCODED onto the wire this tick, under its `feed-author:<feed_key>`
    /// consumer id, keyed by `tick_rev`.
    ///
    /// Called from inside the typed-producer closure (which the structural-pairing
    /// registration helper installs) at the moment it materializes the window it
    /// emits. When `tick_rev` advances the sink is cleared first, so a feed that
    /// stops emitting drops out of the guardrail's view. A poisoned mutex (D6) is
    /// a silent no-op (the guardrail simply has nothing to check for that feed).
    pub fn record_emitted_feed_authors(
        &self,
        tick_rev: u64,
        consumer_id: impl Into<String>,
        authors: impl IntoIterator<Item = String>,
    ) {
        record_emitted_feed_authors(&self.emitted_feed_authors, tick_rev, consumer_id, authors);
    }

    /// ADR-0070 D7 (#1671 Lane H) — a clone of the emitted-author sink handle, for
    /// a typed-producer closure to write to WITHOUT re-locking the registry (it
    /// runs inside `run_typed()` while the registry mutex is held). Write through
    /// the free function [`record_emitted_feed_authors`].
    #[must_use]
    pub fn emitted_feed_authors_handle(&self) -> EmittedFeedAuthorsSlot {
        Arc::clone(&self.emitted_feed_authors)
    }

    /// ADR-0070 D7 (#1671 Lane H) — drain the emitted-author sink for the
    /// structural guardrail (BLOCKING 2).
    ///
    /// Returns `(consumer_id, author_key)` pairs for every author a feed encoded
    /// onto the wire on the tick matching `tick_rev`. The kernel reads this AFTER
    /// the typed projections are emitted and warns for any pair with no live
    /// resolver demand. A stale rev (the sink was never written this tick) yields
    /// nothing. A poisoned mutex (D6) yields nothing.
    #[must_use]
    pub fn emitted_feed_authors_for_tick(&self, tick_rev: u64) -> Vec<(String, String)> {
        match self.emitted_feed_authors.lock() {
            Ok(guard) if guard.0 == tick_rev => guard
                .1
                .iter()
                .flat_map(|(consumer, keys)| {
                    keys.iter().map(move |k| (consumer.clone(), k.clone()))
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// ADR-0070 D7 (#1671 Lane H) — run ONE registered feed-author provider by
    /// key and return its current visible-author set (test introspection).
    ///
    /// Returns an empty vec when no provider is registered under `feed_key`. A
    /// panicking provider is swallowed (D6), same as [`Self::run_feed_author_providers`].
    #[must_use]
    pub fn run_feed_author_provider(&self, feed_key: &str) -> Vec<String> {
        match self.feed_author_providers.get(feed_key) {
            Some(provider) => catch_unwind(AssertUnwindSafe(provider)).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Run every registered feed-author provider and return `(feed_key, keys)`
    /// snapshots for this tick.
    ///
    /// Mirrors `run_typed`'s D6/D8 contract: each closure runs on the actor
    /// thread inside `make_update` and MUST be non-blocking; a panicking provider
    /// is swallowed inside [`catch_unwind`] (its feed simply contributes no
    /// authors this tick — the same shape as an unregistered provider). The kernel
    /// then reconciles each `(feed_key, keys)` against its prior set.
    pub fn run_feed_author_providers(&self) -> Vec<(String, Vec<String>)> {
        let mut out = Vec::with_capacity(self.feed_author_providers.len());
        for (feed_key, provider) in &self.feed_author_providers {
            match catch_unwind(AssertUnwindSafe(provider)) {
                Ok(keys) => out.push((feed_key.clone(), keys)),
                Err(_) => continue,
            }
        }
        out
    }
}
