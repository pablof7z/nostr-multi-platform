//! ADR-0063 D7 (#1671 Lane H) — the structural feed-author auto-resolve pairing
//! seam, extracted from `snapshot.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md file-size rule). These are inherent [`NmpApp`] methods, so
//! the mount point (`#[path]` from `snapshot.rs`) is irrelevant to callers.

use super::NmpApp;

impl NmpApp {
    /// ADR-0063 D7 (#1671 Lane H) — register the feed-author-set provider for a
    /// feed snapshot key (e.g. `"nmp.feed.home"`).
    ///
    /// `f` returns the raw author keys the feed will RENDER for its CURRENT
    /// visible window; the kernel calls it INSIDE every snapshot tick and
    /// auto-`resolve_ref`s the additions / `release_ref`s the removals under the
    /// consumer id `feed-author:<feed_key>`. Prefer [`Self::register_feed_render_source`],
    /// which installs this lane STRUCTURALLY paired with the typed sidecar; this
    /// bare method exists for that helper and for tests.
    ///
    /// `f` runs on the actor thread inside the tick — it MUST be non-blocking
    /// (D8). Last-writer-wins on the key. A poisoned registry mutex is a silent
    /// no-op (D6). Removed by [`Self::unregister_feed`].
    pub fn register_feed_author_provider(
        &self,
        feed_key: impl Into<String>,
        f: impl Fn() -> Vec<String> + Send + Sync + 'static,
    ) {
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            registry.register_feed_author_provider(feed_key, f);
        }
    }

    /// ADR-0063 D7 (#1671 Lane H, BLOCKING 1) — register a feed's typed sidecar
    /// AND its feed-author auto-resolve provider from ONE source, STRUCTURALLY
    /// paired.
    ///
    /// This is THE registration seam for any feed whose rows carry authors. It
    /// installs both lanes from the SAME [`nmp_feed::FeedRenderSource`], so it is
    /// IMPOSSIBLE to register a feed's typed sidecar without also registering the
    /// author provider that auto-resolves the authors that sidecar renders — the
    /// coverage hole (a dynamic feed that emits a typed sidecar but no provider →
    /// blank avatars) cannot recur, because there is no code path that registers
    /// one half alone.
    ///
    /// The two installed closures share `source`'s per-tick window memo: the
    /// kernel publishes ONE per-tick rev at the top of `make_update`; the provider
    /// (run first) materializes the window at that rev and the typed producer (run
    /// later in the same tick) reuses the SAME materialization. So the author set
    /// resolved == the window emitted, even if a concurrent `load_older` widens
    /// the live window between the two reads (the HIGH 1-frame gap is closed).
    ///
    /// The typed producer also records the authors it ENCODES into the kernel's
    /// emitted-author sink (BLOCKING 2): the structural guardrail then warns
    /// (debug-only) for any emitted author with no live resolver demand.
    ///
    /// - `feed_key` — the feed snapshot key (`"nmp.feed.home"`,
    ///   `"nmp.feed.author.<pk>"`, `"nmp.feed.thread.<id>"`). Both lanes key on it,
    ///   so [`NmpApp::unregister_feed`] tears BOTH down together.
    /// - `source` — the per-tick-materialized window over the feed engine.
    /// - `encode` — maps the materialized snapshot to the typed sidecar payload
    ///   (`None` to omit this tick, e.g. an unchanged frame under incremental
    ///   apply). Receives the SAME `&S` the provider derived authors from.
    pub fn register_feed_render_source<S>(
        &self,
        feed_key: impl Into<String>,
        source: std::sync::Arc<nmp_feed::FeedRenderSource<S>>,
        encode: impl Fn(&S) -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    ) where
        S: nmp_feed::FeedAuthorRefs + Send + Sync + 'static,
    {
        use std::sync::atomic::Ordering;
        let feed_key = feed_key.into();
        let consumer_id = format!("feed-author:{feed_key}");
        let tick_rev = self.frame_tick_rev_handle();
        let emitted_sink = self.emitted_feed_authors_handle();

        // ── Typed-sidecar lane ────────────────────────────────────────────────
        let source_for_typed = std::sync::Arc::clone(&source);
        let tick_rev_for_typed = std::sync::Arc::clone(&tick_rev);
        let consumer_for_typed = consumer_id;
        self.register_typed_snapshot_projection(feed_key.clone(), move || {
            let rev = tick_rev_for_typed.load(Ordering::Acquire);
            // The SAME per-tick materialization the provider used (no gap).
            let snapshot = source_for_typed.snapshot_for_tick(rev);
            // BLOCKING 2 — record what actually crosses the wire so the structural
            // guardrail can compare it against live resolver demand. Write through
            // the captured sink handle, NOT the registry (run_typed holds that
            // mutex; re-locking would deadlock).
            nmp_core::record_emitted_feed_authors(
                &emitted_sink,
                rev,
                consumer_for_typed.clone(),
                snapshot.visible_author_keys(),
            );
            encode(&snapshot)
        });

        // ── Author-provider lane (structurally paired) ────────────────────────
        let source_for_provider = source;
        let tick_rev_for_provider = tick_rev;
        self.register_feed_author_provider(feed_key, move || {
            let rev = tick_rev_for_provider.load(Ordering::Acquire);
            source_for_provider.author_keys_for_tick(rev)
        });
    }

    /// ADR-0063 D7 (#1671 Lane H) — the feed-author-provider keys currently
    /// registered, without running any provider. Use in structural-pairing tests
    /// to assert that registering a feed's typed sidecar ALSO registered its
    /// author provider under the same key (and that closing the feed removes both).
    #[must_use]
    pub fn registered_feed_author_provider_keys(&self) -> Vec<String> {
        self.snapshot_projections
            .lock()
            .map(|registry| {
                registry
                    .registered_feed_author_provider_keys()
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// ADR-0063 D7 (#1671 Lane H) — run ONE feed-author provider by key and
    /// return its current visible-author set (test introspection: prove a
    /// dynamic feed's rendered authors are surfaced for auto-resolve).
    #[must_use]
    pub fn run_feed_author_provider_for_test(&self, feed_key: &str) -> Vec<String> {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.run_feed_author_provider(feed_key))
            .unwrap_or_default()
    }
}
