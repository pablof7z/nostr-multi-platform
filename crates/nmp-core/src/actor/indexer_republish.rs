//! Indexer-republish pipeline — passive gossip replication.
//!
//! When NMP receives a NIP-01 *replaceable* event (kind 0, kind 3, or any
//! kind in 10000–19999) from a non-indexer relay, this pipeline forwards
//! the verified event verbatim — wrapped in a plain `["EVENT", <event>]`
//! frame — to every currently-connected indexer relay.
//!
//! The motivation is pragmatic: indexer relays (e.g. `purplepag.es`) are
//! how unrelated apps cold-start a pubkey's profile / contact list / inbox
//! mailbox. A stale answer at the indexer poisons every downstream cold-
//! start. NMP already pays the socket cost to talk to an indexer (lane
//! discriminator `RelayRole::Indexer`); the marginal cost of one extra
//! `EVENT` frame per replaceable arrival is near-zero, and the payoff is
//! that the gossip layer self-heals: whenever NMP fetches a fresher kind:0
//! from an author's own write relay, it keeps the indexer honest.
//!
//! ## Scope discipline
//!
//! - **Passive replication only.** NOT a substitute for [`crate::publish`]
//!   `PublishEngine` — no ack tracking, no `OK: false` retries, no outbox
//!   resolution. Fire-and-forget.
//! - **Replaceable kinds only.** Reuses
//!   [`crate::store::RawEvent::is_replaceable`] verbatim:
//!   `kind == 0 || kind == 3 || (10_000..20_000).contains(&kind)`.
//!   Parameterized replaceables (30000–39999) are deliberately out of scope
//!   — the dedup key needs `d`-tag normalization and indexer relevance is
//!   weaker.
//! - **Loop prevention is structural.** Two skip rules cover every cycle:
//!     1. *Source-is-indexer skip* — if the delivering relay is already an
//!        indexer we forward nothing (the indexer → other-indexer relay
//!        link is the upstream gossip layer's responsibility, not ours).
//!     2. *Indexer-provenance skip* — if any indexer is already in the
//!        store's [`crate::store::EventStore::provenance_for`] list, we
//!        skip — the indexer already has this `id` and an `EVENT` frame
//!        would be a wasted socket write.
//! - **Bounded in-memory dedup.** A `VecDeque<(EventId, RelayUrl)>` plus
//!   companion `HashSet` deduplicates within a session, capped at 4096
//!   entries. Per-session in-memory only: indexers de-dup on `id`, so the
//!   restart cost is at most one redundant frame per pair.
//!
//! ## D0 compliance
//!
//! This module sits in `nmp-core` because indexer-lane discrimination
//! (`RelayRole::Indexer`, `IndexerRelaysSlot`) is already a substrate
//! concept, and "republish a verbatim signed event to a relay" is a
//! relay-management primitive — not a NIP-specific operation. The module
//! names no NIP and no protocol noun beyond the already-substrate
//! `RelayRole::Indexer` discriminator. D0-clean.
//!
//! ## Wiring
//!
//! [`IndexerRepublishPipeline`] is constructed inside the actor entry point
//! (`run_actor_with_observers`) once `Pool`, the `IndexerRelaysSlot`, and
//! the kernel's `EventStore` handle are all in scope. It is registered as
//! a typed `RawEventObserver` via
//! [`crate::actor::register_rust_raw_observer`] with a kind filter that
//! matches kinds 0, 3, and 10000–19999. The observer fires on the actor
//! thread after the kernel's existing Schnorr + id-hash gate has accepted
//! the event AND the store has recorded provenance for it (see
//! `kernel/ingest/mod.rs::verify_and_persist` and
//! `kernel/ingest/timeline.rs::ingest_event`).

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use crate::actor::{
    register_rust_raw_observer, unregister_raw_observer, KindFilter, RawEventObserver,
    RawEventObserverId, RawEventObserverSlot,
};
use crate::kernel::{IndexerRelaysSlot, Kernel};
use crate::store::{EventStore, RawEvent};

use nmp_network::pool::{Pool, WireFrame};

/// Actor-local slot that holds the currently-installed
/// [`IndexerRepublishPipeline`]'s observer id, if any. Set when the pipeline
/// is registered; consulted by the Reset arm so the stale registration
/// (which holds an `Arc` to the now-discarded kernel's `EventStore` /
/// `IndexerRelaysSlot`) can be unregistered before a fresh pipeline is
/// installed against the rebuilt kernel.
///
/// `None` means no pipeline is installed (e.g. between construction and the
/// first `run_actor_with_observers` call, or after an `unregister` that
/// nothing has replaced yet).
pub(crate) type IndexerRepublishObserverIdSlot = Arc<Mutex<Option<RawEventObserverId>>>;

/// Construct a fresh, empty [`IndexerRepublishObserverIdSlot`]. Mirrors the
/// shape of [`crate::slots::new_dm_inbox_observer_id_slot`].
#[must_use]
pub(crate) fn new_indexer_republish_observer_id_slot() -> IndexerRepublishObserverIdSlot {
    Arc::new(Mutex::new(None))
}

/// Install (or re-install) the [`IndexerRepublishPipeline`] against the
/// supplied [`Kernel`]'s current handles. Idempotent across kernel
/// rebuilds: if `id_slot` already carries an observer id from a previous
/// registration, that registration is unregistered first so the slot ends
/// up holding exactly one pipeline observer pinned to the live kernel.
///
/// Called from two sites:
///
/// 1. `run_actor_with_observers` (initial construction) — `id_slot` is
///    empty; the unregister call is a no-op.
/// 2. `ActorCommand::Reset` (dispatch.rs) — `id_slot` holds the id of the
///    pipeline that was bound to the now-discarded kernel; that observer
///    is unregistered, and a fresh pipeline is registered against the
///    rebuilt kernel's `event_store_handle` / `indexer_relays_handle`.
///
/// The `Pool` clone is process-lifetime (it owns the per-URL worker
/// threads via its inner `Arc<Mutex<PoolInner>>`); the same clone is
/// captured into the new sender on each call.
pub(crate) fn register_indexer_republish_pipeline(
    kernel: &Kernel,
    raw_event_observers: &RawEventObserverSlot,
    pool: &Pool,
    id_slot: &IndexerRepublishObserverIdSlot,
) {
    // Drop the previous registration (if any) so we never accumulate
    // pipelines pointing at dead kernels. `unregister_raw_observer` is a
    // silent no-op for unknown ids (D6).
    let previous_id = id_slot
        .lock()
        .ok()
        .and_then(|mut guard| guard.take());
    if let Some(id) = previous_id {
        unregister_raw_observer(raw_event_observers, id);
    }
    let sender = Arc::new(PoolSender::new(pool.clone())) as Arc<dyn IndexerForwardSender>;
    let pipeline = Arc::new(IndexerRepublishPipeline::new(
        kernel.indexer_republish_enabled(),
        kernel.indexer_relays_handle(),
        kernel.event_store_handle(),
        sender,
    ));
    let new_id = register_rust_raw_observer(
        raw_event_observers,
        replaceable_kind_filter(),
        pipeline,
    );
    // D6: a poisoned slot drops the id-store but leaves the registration
    // intact; the next Reset arm will see `None` and skip the unregister.
    if let Ok(mut guard) = id_slot.lock() {
        *guard = Some(new_id);
    }
}

/// Per-session bound on the dedup cache. Each entry is roughly
/// `64-byte event-id hex + ~40-byte URL + container overhead ≈ 128 B`, so
/// the worst-case footprint is on the order of 512 KB. That sits well under
/// the kind of allocation budget the actor thread already absorbs for
/// store-side LRU caches and view projections.
const DEDUP_CAPACITY: usize = 4096;

/// Compose the kind filter the [`IndexerRepublishPipeline`] registers under:
/// kinds 0 and 3 (universal indexer payloads), plus every kind in the
/// NIP-01 replaceable range 10000–19999 (mailbox/list metadata).
///
/// Returned as an owned [`KindFilter`] so the actor wiring site can hand it
/// straight to [`crate::actor::register_rust_raw_observer`].
#[must_use]
pub(crate) fn replaceable_kind_filter() -> KindFilter {
    let kinds = std::iter::once(0u32)
        .chain(std::iter::once(3u32))
        .chain(10_000u32..20_000u32);
    KindFilter::from_kinds(kinds)
}

/// Send mechanism the pipeline uses to push a verbatim `["EVENT", ...]`
/// frame at a specific indexer URL. Trait-shaped so unit tests can capture
/// outbound frames without standing up a real [`Pool`].
///
/// The production impl ([`PoolSender`]) wraps `nmp_network::pool::Pool`:
/// every send calls [`Pool::ensure_open_with_role`] to obtain (or reopen)
/// a [`nmp_network::pool::RelayHandle`] for the URL on the
/// [`nmp_network::RelayRole::Indexer`] lane, then [`Pool::send`]s a
/// [`WireFrame::Text`].
pub(crate) trait IndexerForwardSender: Send + Sync {
    /// Push `frame_text` (a complete `["EVENT", <event>]` JSON string) at
    /// the relay identified by `url`. Returns `true` iff the worker
    /// accepted the frame; the caller treats `false` as "drop and move on".
    fn send_to(&self, url: &str, frame_text: &str) -> bool;
}

/// Production [`IndexerForwardSender`] backed by [`Pool`]. Holds an owned
/// `Pool` clone — `Pool` is `Arc<Mutex<PoolInner>>` inside (see
/// `nmp_network::pool::Pool`), so the clone is cheap and the inner
/// reference-count keeps the worker threads alive for the actor's lifetime.
pub(crate) struct PoolSender {
    pool: Pool,
}

impl PoolSender {
    #[must_use]
    pub(crate) fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl IndexerForwardSender for PoolSender {
    fn send_to(&self, url: &str, frame_text: &str) -> bool {
        let handle = self
            .pool
            .ensure_open_with_role(&url.to_string(), nmp_network::RelayRole::Indexer);
        self.pool
            .send(handle, WireFrame::Text(frame_text.to_string()))
    }
}

/// Indexer-republish pipeline state. Registered as a typed
/// [`RawEventObserver`]; one instance per actor lifetime (survives `Reset`
/// the same way the rest of the raw-observer slot does — see
/// `kernel/raw_event_observer.rs::take_raw_event_observers_handle_for_reset`).
pub(crate) struct IndexerRepublishPipeline {
    /// Master switch. Wired from kernel configuration; `false` makes
    /// [`RawEventObserver::on_raw_event_with_source`] a zero-allocation
    /// no-op.
    enabled: bool,
    /// Shared snapshot of the user's configured indexer relays. The actor
    /// (sole writer, D4) republishes the URL list on every relay-config
    /// mutation; we lock-read-clone on each fire.
    indexer_relays: IndexerRelaysSlot,
    /// Shared `EventStore` handle. Used purely for
    /// [`EventStore::provenance_for`] — we never write through it.
    store: Arc<dyn EventStore>,
    /// Bounded dedup cache. A `(event_id, indexer_url)` pair only forwards
    /// once per session; subsequent fires are silent no-ops.
    ///
    /// Implemented as `VecDeque` + `HashSet`: the `VecDeque` preserves
    /// insertion order so the oldest entry can be evicted on overflow in
    /// `O(1)`; the `HashSet` answers membership in `O(1)`. Both data
    /// structures are kept in lock-step under one mutex.
    dedup: Mutex<DedupCache>,
    /// Send mechanism — production wraps [`Pool`], tests inject a capture.
    sender: Arc<dyn IndexerForwardSender>,
}

/// Twin-container bounded dedup cache.
struct DedupCache {
    order: VecDeque<(String, String)>,
    set: HashSet<(String, String)>,
}

impl DedupCache {
    fn new() -> Self {
        Self {
            order: VecDeque::with_capacity(DEDUP_CAPACITY),
            set: HashSet::with_capacity(DEDUP_CAPACITY),
        }
    }

    /// Insert `key` if absent; returns `true` iff the caller should forward.
    /// On overflow the oldest entry is evicted in `O(1)`.
    fn insert(&mut self, key: (String, String)) -> bool {
        if self.set.contains(&key) {
            return false;
        }
        if self.order.len() >= DEDUP_CAPACITY {
            if let Some(victim) = self.order.pop_front() {
                self.set.remove(&victim);
            }
        }
        self.set.insert(key.clone());
        self.order.push_back(key);
        true
    }
}

impl IndexerRepublishPipeline {
    /// Construct a new pipeline. `enabled` controls whether the observer
    /// does any work; the rest of the slot wiring runs unconditionally so
    /// a runtime toggle (if one is ever added) can flip the flag without
    /// rebuilding the kernel.
    pub(crate) fn new(
        enabled: bool,
        indexer_relays: IndexerRelaysSlot,
        store: Arc<dyn EventStore>,
        sender: Arc<dyn IndexerForwardSender>,
    ) -> Self {
        Self {
            enabled,
            indexer_relays,
            store,
            dedup: Mutex::new(DedupCache::new()),
            sender,
        }
    }

    /// Core decision + dispatch loop. Pulled out of the trait impl so unit
    /// tests can drive it without serializing/re-decoding the verbatim
    /// JSON the actor would normally hand us.
    ///
    /// Returns the number of indexer URLs the frame was actually sent to.
    /// Tests assert on this count; production ignores the return value.
    pub(crate) fn process(
        &self,
        raw: &RawEvent,
        source_relay_url: Option<&str>,
        verbatim_json: &str,
    ) -> usize {
        if !self.enabled {
            return 0;
        }
        if !raw.is_replaceable() {
            return 0;
        }
        // Snapshot the configured indexer URLs.
        let indexer_urls: Vec<String> = match self.indexer_relays.lock() {
            Ok(guard) => guard.as_slice().to_vec(),
            Err(_) => return 0, // D6: poisoned slot is a silent no-op.
        };
        if indexer_urls.is_empty() {
            return 0;
        }
        // Loop-prevention rule 1: never forward an event back to the lane
        // that sent it to us. If the source is an indexer we stop here —
        // cross-indexer propagation is the upstream gossip layer's job.
        //
        // String-equality match on URLs assumes both sides share the same
        // canonicalization. Today that holds in practice — the indexer
        // slot is populated from `RelayEditRow.url` (the configured URL)
        // and ingest provenance is set from the pool's `Opened` URL
        // (also the configured URL post-`CanonicalRelayUrl::parse`); a
        // user who edits a relay row with mixed case is a pre-existing
        // hazard shared with `nmp_router::Nip65OutboxResolver`. A
        // canonicalize-before-compare hardening pass would belong on
        // `RelayUrls::as_slice` so every consumer benefits at once.
        if let Some(source) = source_relay_url {
            if indexer_urls.iter().any(|u| u == source) {
                return 0;
            }
        }
        // Loop-prevention rule 2: if any indexer is already in the store's
        // provenance list, the indexer already has this `id` and a republish
        // would be a wasted socket write. Provenance writes happen inside
        // `EventStore::insert` BEFORE the raw observer fires (see
        // `kernel/ingest/mod.rs`), so a prior delivery from indexer X
        // shows up here.
        let event_id_bytes = raw.id_bytes();
        let provenance_indexer_match = match self.store.provenance_for(&event_id_bytes) {
            Ok(entries) => entries
                .iter()
                .any(|entry| indexer_urls.iter().any(|u| *u == entry.relay_url)),
            Err(_) => false, // D6: store read failure → behave as "not yet seen".
        };
        if provenance_indexer_match {
            return 0;
        }
        // Build the wire frame once. We re-serialize via the verbatim JSON
        // the kernel already produced for the observer; the indexer
        // verifies the signature, byte-equality with the upstream relay's
        // wire payload is irrelevant.
        let frame_text = format!(r#"["EVENT",{verbatim_json}]"#);
        let mut sent = 0usize;
        for target in &indexer_urls {
            // Source URL is structurally never in the forward set here
            // (rule 1 above already short-circuited that case), but
            // defense-in-depth: skip explicitly anyway.
            if source_relay_url.is_some_and(|s| s == target.as_str()) {
                continue;
            }
            let key = (raw.id.clone(), target.clone());
            let should_forward = match self.dedup.lock() {
                Ok(mut guard) => guard.insert(key),
                Err(_) => false, // D6: poisoned dedup → drop rather than double-send.
            };
            if !should_forward {
                continue;
            }
            if self.sender.send_to(target, &frame_text) {
                sent = sent.saturating_add(1);
            }
        }
        sent
    }
}

impl RawEventObserver for IndexerRepublishPipeline {
    fn on_raw_event(&self, _kind: u32, _json: &str) {
        // The replaceable-kind decision and provenance check both need the
        // structured `RawEvent` + the source URL, not just the serialized
        // JSON. The kernel always invokes `on_raw_event_with_source` (see
        // `actor/commands/raw_event_observer.rs::notify_raw_observers`); the
        // bare entry point exists only to satisfy the trait's required
        // method and is a deliberate no-op here.
    }

    fn on_raw_event_with_source(
        &self,
        _kind: u32,
        json: &str,
        source_relay_url: Option<&str>,
    ) {
        // The kernel hands us the verbatim NIP-01 JSON. Decode just enough
        // to reuse `RawEvent::is_replaceable` / `id_bytes`; the kernel's
        // Schnorr + id-hash gate has already validated the event so this
        // is purely a shape decode, not a security boundary.
        let Ok(raw) = serde_json::from_str::<RawEvent>(json) else {
            return; // D6: malformed JSON → silent no-op.
        };
        let _ = self.process(&raw, source_relay_url, json);
    }
}

#[cfg(test)]
#[path = "indexer_republish/tests.rs"]
mod tests;
