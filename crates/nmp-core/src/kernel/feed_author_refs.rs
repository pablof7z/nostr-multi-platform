//! ADR-0070 D7 (#1671 Lane H) — feed-author auto-resolve + debug guardrail.
//!
//! Closes the coverage hole: any author a feed RENDERS must resolve through the
//! SAME `resolve_ref` path automatically, so a shell can't silently forget and
//! render a blank avatar. The framework owns the wiring, not the shell.
//!
//! ## In-tick, not next-tick (the 1-frame-gap decision)
//!
//! The reconcile runs INSIDE [`Kernel::make_update`] with `&mut self`
//! ([`Kernel::reconcile_feed_author_refs`]), NOT as a next-tick actor command.
//! A next-tick `ActorCommand::ResolveRef` would land the resolve one frame after
//! the row first appears — a 1-frame window where the row is on screen but its
//! profile interest is not yet registered (the exact blank-avatar gap D7
//! forbids). Running in-tick is possible because the visible-author set is
//! gathered from `Fn() -> Vec<String>` provider closures (which only READ the
//! engine's current window), so the kernel collects the sets first (dropping the
//! registry lock) and THEN mutates its own resolver state with `&mut self` — no
//! self-borrow conflict, no actor round-trip.
//!
//! ## Consumer-id scheme (dedup with explicit claims)
//!
//! Each feed reconciles under `feed-author:<feed_key>`:
//! `feed-author:microblog.timeline.home`,
//! `feed-author:microblog.feed.author.<pk>`,
//! `feed-author:test.feed.thread.<id>`. `feed_key` is an app-owned dynamic
//! projection key (`DynamicProjectionKey::app_owned`, PR #2610), never
//! `nmp.*`. Because the auto-resolve goes through the SAME `resolve_ref` seam
//! as an explicit open-profile-screen claim, the two share one per-pubkey
//! resolver slot: the feed holds `Ref`/`CacheOk`, the open
//! screen holds `Card`/`Live`, and Lane B's widen + "Live wins" rules keep the
//! slot at the widest/liveliest demand while BOTH consumers hold it. Releasing
//! one consumer narrows but does not tear down the slot until the last releases.
//!
//! ## Trellis-backed diff (#3116)
//!
//! The per-consumer added/removed diff is NOT hand-rolled: the kernel owns one
//! session-persistent
//! [`nmp_core::trellis_reconciler::KeyedReconciler`]`<(String, String), ()>`
//! ([`Kernel::feed_author_reconciler`]) keyed by `(consumer_id, author_key)` —
//! the composite key is load-bearing, not incidental, because it is what lets
//! ONE reconciler replace what used to be an independent `SetDiff` per feed
//! consumer: two consumers both demanding the same author are two distinct
//! keys, so consumer A dropping the author closes ONLY `(A, author)` while
//! `(B, author)` stays open — `resolve_profile_ref`/`release_ref`'s own
//! `profile_claims` refcount (not this reconciler) is where cross-consumer
//! union/dedup already lives (see "dedup with explicit claims" above).
//! [`Kernel::reconcile_feed_author_refs`] feeds the reconciler the FULL
//! current desired map every tick; a consumer whose provider vanished or
//! whose visible set went empty simply contributes zero keys, so Trellis's
//! diff closes every pair it previously held — the old explicit "stale
//! consumer" sweep is gone, subsumed by feeding Trellis the complete map.
//! [`Kernel::apply_feed_author_ref_commands`] is the only place the returned
//! plan turns into real `resolve_profile_ref`/`release_ref` calls, applied
//! **in-tick** (`&mut self`, same frame — trellis-core's diff is pure/data-only
//! so it composes with the in-tick constraint above) and in `Vec` order
//! (never sorted/parallelized — LIFO close-vs-close correctness lives in that
//! order). `auto_profile_refs_by_consumer` is kept mirroring the reconciler's
//! now-committed state so the debug guardrails below (which read it directly)
//! stay accurate.

use std::collections::{BTreeMap, BTreeSet};

use trellis_core::{ResourceCommand, ResourceKey};

use super::refs::{ProfileShape, RefLiveness, RefNamespace};
use super::{Kernel, OutboundMessage};

/// Trellis-internal diagnostic scope label — never surfaced to a concept.
pub(in crate::kernel) const FEED_AUTHOR_RECONCILER_SCOPE: &str =
    "nmp.core.kernel.feed-author-refs.v1";

/// Derives the composite `(consumer_id, author_key)` `ResourceKey` — ordered
/// SEGMENTS, not a delimited string, so [`feed_author_key_parts`] recovers
/// each part exactly from a Trellis `Open`/`Close` command without parsing
/// (`ResourceKey::from_segments` — #3116).
pub(in crate::kernel) fn feed_author_resource_key(key: &(String, String)) -> ResourceKey {
    let (consumer_id, author_key) = key;
    ResourceKey::from_segments([consumer_id.as_str(), author_key.as_str()])
}

/// Recovers `(consumer_id, author_key)` from a resource key built by
/// [`feed_author_resource_key`].
fn feed_author_key_parts(key: &ResourceKey) -> (String, String) {
    assert_eq!(
        key.segment_count(),
        2,
        "feed-author resource keys always carry exactly two segments \
         (consumer_id, author_key) — built only by `feed_author_resource_key`"
    );
    (
        key.segment(0).unwrap_or_default().to_string(),
        key.segment(1).unwrap_or_default().to_string(),
    )
}

/// The consumer-id prefix for every feed-author auto-resolve claim (D7).
pub(crate) const FEED_AUTHOR_CONSUMER_PREFIX: &str = "feed-author:";

/// Build the auto-resolve consumer id for a feed snapshot key.
///
/// `feed-author:microblog.timeline.home` for an app-owned timeline feed;
/// `feed-author:microblog.feed.author.<pk>` / `feed-author:test.feed.thread.<id>`
/// for the transient author/thread feeds (whose `feed_key` already carries
/// the per-screen suffix). `feed_key` is always app-owned
/// (`DynamicProjectionKey::app_owned`) — never `nmp.*`.
#[must_use]
pub(crate) fn feed_author_consumer_id(feed_key: &str) -> String {
    format!("{FEED_AUTHOR_CONSUMER_PREFIX}{feed_key}")
}

impl Kernel {
    /// Reconcile every registered feed's CURRENT visible-author set against the
    /// prior tick (ADR-0070 D7). Called once per snapshot tick from
    /// [`Kernel::make_update`], BEFORE the typed projections are emitted, so the
    /// auto-resolve lands in the SAME frame the row appears (no blank-avatar
    /// gap).
    ///
    /// Feeds [`Kernel::feed_author_reconciler`] the FULL desired
    /// `(consumer_id, author_key)` map for this tick and applies the returned
    /// plan via [`Kernel::apply_feed_author_ref_commands`]:
    /// - ADDED pairs (in the new map, not the prior) → `resolve_ref`
    ///   (Profile, key, consumer, `Ref`, `CacheOk`) — the feed-avatar shape at
    ///   cache freshness (no per-row tailing sub).
    /// - REMOVED pairs (in the prior map, not the new) → `release_ref`.
    ///
    /// A consumer whose provider vanished this tick, or whose visible set went
    /// empty, contributes zero pairs — Trellis's diff closes every pair it
    /// previously held, so no separate "stale consumer" sweep is needed (see
    /// module docs).
    ///
    /// Returns the outbound messages the resolves/releases produced (REQ
    /// registrations, etc.) so `make_update`'s caller can route them, exactly
    /// like a direct `resolve_ref` call.
    pub(in crate::kernel) fn reconcile_feed_author_refs(&mut self) -> Vec<OutboundMessage> {
        let sets = self.collect_feed_author_sets();
        // Track which consumers still have a live provider this tick, for the
        // debug guardrail below (independent of whether its visible set is
        // currently empty).
        let mut live_consumers: BTreeSet<String> = BTreeSet::new();
        let mut desired: BTreeMap<(String, String), ()> = BTreeMap::new();
        for (feed_key, keys) in sets {
            let consumer_id = feed_author_consumer_id(&feed_key);
            live_consumers.insert(consumer_id.clone());
            for key in keys.into_iter().filter(|k| !k.is_empty()) {
                desired.insert((consumer_id.clone(), key), ());
            }
        }
        let commands = self.feed_author_reconciler.reconcile(desired);
        let out = self.apply_feed_author_ref_commands(commands);
        #[cfg(debug_assertions)]
        self.warn_unresolved_feed_authors(&live_consumers);
        out
    }

    /// Applies a Trellis resource plan **in `Vec` order** — never sort or
    /// parallelize; LIFO close-vs-close correctness on scope teardown lives in
    /// this order (#3116 VERIFY-FIRST note). `Open` resolves a
    /// `(consumer_id, author_key)` pair through the SAME `resolve_ref` seam an
    /// explicit claim uses; `Close` releases it. Keeps
    /// `auto_profile_refs_by_consumer` mirroring the reconciler's
    /// now-committed state, since the debug guardrails read it directly.
    fn apply_feed_author_ref_commands(
        &mut self,
        commands: Vec<ResourceCommand<()>>,
    ) -> Vec<OutboundMessage> {
        let mut out = Vec::new();
        for command in commands {
            match command {
                ResourceCommand::Open { key, .. } => {
                    let (consumer_id, author_key) = feed_author_key_parts(&key);
                    out.extend(self.resolve_profile_ref(
                        author_key.clone(),
                        consumer_id.clone(),
                        ProfileShape::Ref,
                        RefLiveness::CacheOk,
                        false,
                        Vec::new(),
                    ));
                    self.auto_profile_refs_by_consumer
                        .entry(consumer_id)
                        .or_default()
                        .insert(author_key);
                }
                ResourceCommand::Close { key, .. } => {
                    let (consumer_id, author_key) = feed_author_key_parts(&key);
                    out.extend(self.release_ref(RefNamespace::Profile, &author_key, &consumer_id));
                    if let Some(keys) = self.auto_profile_refs_by_consumer.get_mut(&consumer_id) {
                        keys.remove(&author_key);
                        if keys.is_empty() {
                            self.auto_profile_refs_by_consumer.remove(&consumer_id);
                        }
                    }
                }
                ResourceCommand::Replace { .. } | ResourceCommand::Refresh { .. } => {
                    // Never emitted: the payload is `()`, so a same-key join
                    // can never carry a changed payload
                    // (`KeyedReconciler::new`'s planner only opens added /
                    // closes removed) — exhaustive match, not a reachable
                    // branch.
                }
            }
        }
        out
    }

    /// Release EVERY auto-resolved ref a feed consumer holds (ADR-0070 D7) and
    /// drop its tracking entry.
    ///
    /// The leak guard: a durable app-owned timeline feed consumer is
    /// the #1 leak risk — without this, its visible-author refs would accumulate
    /// for the life of the process. A shell calls this when the feed closes:
    /// the `unregister_feed` path removes the provider, and the next reconcile
    /// sweep then releases-all here. (A same-call, immediate release for a
    /// transient feed — see [`Self::release_feed_author_refs_for_feed`] — is
    /// staged for a later lane's `ActorCommand` teardown and has no live call
    /// site yet.) Returns the release outbound messages. Idempotent — a
    /// consumer with no tracked refs is a no-op.
    ///
    /// Rebuilds the FULL desired map minus `consumer_id`'s own entries and
    /// reconciles the (persistent) [`Kernel::feed_author_reconciler`] against
    /// it — every other consumer's pairs are untouched (same key, same `()`
    /// payload, so Trellis's diff is a no-op for them) and Trellis emits
    /// exactly the `Close` commands for `consumer_id`'s pairs.
    pub(in crate::kernel) fn release_all_feed_author_refs(
        &mut self,
        consumer_id: &str,
    ) -> Vec<OutboundMessage> {
        if !self.auto_profile_refs_by_consumer.contains_key(consumer_id) {
            return Vec::new();
        }
        let desired: BTreeMap<(String, String), ()> = self
            .auto_profile_refs_by_consumer
            .iter()
            .filter(|(c, _)| c.as_str() != consumer_id)
            .flat_map(|(c, keys)| keys.iter().map(move |k| ((c.clone(), k.clone()), ())))
            .collect();
        let commands = self.feed_author_reconciler.reconcile(desired);
        self.apply_feed_author_ref_commands(commands)
    }

    /// Release-all by FEED key — the immediate-teardown seam (translate feed key
    /// → consumer id, delegate). Exercised by this module's tests; the wired
    /// `unregister_feed` path drops the provider and lets the next-tick reconcile
    /// sweep release-all, so the kernel-only Lane H has no direct caller yet — a
    /// later lane's `ActorCommand` teardown will call this for same-call release.
    // `allow(dead_code)`: exercised in this module's tests; production caller
    // lands with the actor teardown seam (Lane H, no live call site yet).
    #[allow(dead_code)]
    pub(in crate::kernel) fn release_feed_author_refs_for_feed(
        &mut self,
        feed_key: &str,
    ) -> Vec<OutboundMessage> {
        let consumer_id = feed_author_consumer_id(feed_key);
        self.release_all_feed_author_refs(&consumer_id)
    }

    /// DEBUG GUARDRAIL (ADR-0070 D7, BLOCKING 2) — warn when an author a feed's
    /// typed producer ACTUALLY EMITTED onto the wire this tick has NO live
    /// resolver demand.
    ///
    /// This is the STRUCTURAL guardrail: it does NOT iterate the kernel's own
    /// provider-tracking ([`Self::warn_unresolved_feed_authors`] does that — a
    /// reconciled-key-lost-demand check). It instead reads the author keys the
    /// feed's typed producer recorded as crossing the wire (the EMITTED window)
    /// and compares them against live demand. A feed that emits a pubkey it never
    /// resolved — because it skipped the structural-pairing helper, OR because its
    /// provider's `FeedAuthorRefs` set missed a field the row actually renders —
    /// is therefore caught even though it is INVISIBLE to the provider-tracking
    /// check (it has no `auto_profile_refs_by_consumer` entry for that key).
    ///
    /// Debug-only and demand-checked (never content-checked): a freshly-resolved
    /// author whose `kind:0` has not arrived has empty content but `Some` demand,
    /// so it is NOT flagged — only a genuinely unresolved emitted author is.
    ///
    /// Called from `make_update` AFTER the typed projections are emitted (so the
    /// sink is populated), with the current tick rev.
    #[cfg(debug_assertions)]
    pub(in crate::kernel) fn warn_emitted_unresolved_feed_authors(&self, tick_rev: u64) {
        for (consumer_id, key) in self.emitted_unresolved_feed_authors(tick_rev) {
            tracing::warn!(
                target: "nmp.refs.guardrail",
                consumer = %consumer_id,
                pubkey = %super::short_hex(&key),
                "ADR-0070 D7 guardrail: feed EMITTED author onto the wire with NO \
                 live resolver demand — a sidecar crossed a pubkey that was never \
                 resolve_ref-d (missed provider, or a FeedAuthorRefs field the \
                 provider's author set didn't cover; NOT the normal empty-profile \
                 async gap)"
            );
        }
    }

    /// The emitted-set guardrail CONDITION, factored out so it is testable: every
    /// `(consumer_id, author_key)` the feeds' typed producers recorded as EMITTED
    /// on the tick matching `tick_rev` for which the unified resolver records NO
    /// live demand ([`Kernel::ref_demanded_profile_shape`] is `None`).
    ///
    /// With the structural-pairing helper wired this is ALWAYS empty: every
    /// emitted author shares its feed's per-tick materialization with the provider
    /// that just resolved it, so demand is `Some`. It becomes non-empty only when
    /// a surface emits a pubkey WITHOUT routing it through `resolve_ref` — the
    /// regression this guardrail structurally catches.
    #[cfg(any(test, debug_assertions))]
    pub(in crate::kernel) fn emitted_unresolved_feed_authors(
        &self,
        tick_rev: u64,
    ) -> Vec<(String, String)> {
        self.emitted_feed_authors(tick_rev)
            .into_iter()
            .filter(|(_, key)| self.ref_demanded_profile_shape(key).is_none())
            .collect()
    }

    /// DEBUG GUARDRAIL (ADR-0070 D7) — warn when a feed-author this kernel just
    /// reconciled has NO live resolver demand.
    ///
    /// This fires when an EMITTED feed-row author has no resolver slot at all —
    /// i.e. a future feed surface rendered an author WITHOUT routing it through
    /// `resolve_ref` (the auto-resolve helper was bypassed, the reconcile failed
    /// to claim, or the slot was torn down out of band). It does NOT fire merely
    /// because the profile CONTENT is still empty (a fresh `CacheOk` resolve that
    /// hasn't fetched yet) — that is the normal async gap, and the demand
    /// ([`Kernel::ref_demanded_profile_shape`]) is `Some` in that case. The
    /// signal is "rendered author with zero demand," mirroring ADR-0070's
    /// `Undeclared` debug-assert discipline: loud in dev, absent in release.
    #[cfg(debug_assertions)]
    fn warn_unresolved_feed_authors(&self, live_consumers: &BTreeSet<String>) {
        for (consumer_id, key) in self.unresolved_feed_authors(live_consumers) {
            tracing::warn!(
                target: "nmp.refs.guardrail",
                consumer = %consumer_id,
                pubkey = %super::short_hex(&key),
                "ADR-0070 D7 guardrail: feed renders author with NO live resolver \
                 demand — a surface emitted a pubkey without resolve_ref (NOT the \
                 normal empty-profile async gap)"
            );
        }
    }

    /// The guardrail CONDITION, factored out so it is testable (the `warn!`
    /// itself is a side-effect we can't easily assert): every `(consumer_id,
    /// pubkey)` where the feed reconciled that author into its visible set but
    /// the unified resolver records NO live demand for it
    /// ([`Kernel::ref_demanded_profile_shape`] is `None`). With the auto-resolve
    /// helper wired this set is ALWAYS empty (every reconciled author was just
    /// `resolve_ref`-d, so demand is `Some`); it becomes non-empty only when a
    /// surface renders an author WITHOUT routing it through `resolve_ref` — the
    /// future-regression this guardrail exists to catch. Empty-profile-content
    /// (the normal async fetch gap) is NOT flagged: demand is `Some` the instant
    /// the resolve registers, long before the kind:0 arrives.
    #[cfg(any(test, debug_assertions))]
    pub(in crate::kernel) fn unresolved_feed_authors(
        &self,
        live_consumers: &BTreeSet<String>,
    ) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for consumer_id in live_consumers {
            let Some(keys) = self.auto_profile_refs_by_consumer.get(consumer_id) else {
                continue;
            };
            for key in keys {
                if self.ref_demanded_profile_shape(key).is_none() {
                    out.push((consumer_id.clone(), key.clone()));
                }
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "feed_author_refs_support.rs"]
mod feed_author_refs_support;

#[cfg(test)]
#[path = "feed_author_refs_tests.rs"]
mod feed_author_refs_tests;

#[cfg(test)]
#[path = "feed_author_refs_tests_equivalence.rs"]
mod feed_author_refs_tests_equivalence;
