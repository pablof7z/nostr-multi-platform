//! Host-extensible snapshot output — the typed FlatBuffers sidecar seam
//! (ADR-0037).
//!
//! Hosts register typed projection closures whose FlatBuffers bytes are
//! carried in the snapshot frame's `typed_projections` sidecar.  The legacy
//! generic (`serde_json::Value`) lane has been removed; only typed projections
//! remain.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use crate::update_envelope::{TypedProjectionData, WireProjectionState};

/// A host-registered **typed** projection closure — the FlatBuffers-sidecar
/// counterpart to the removed generic `ProjectionFn`.
///
/// Where the old `ProjectionFn` returned a generic `serde_json::Value` appended to
/// `KernelSnapshot::projections`, a `TypedProjectionFn` returns opaque
/// FlatBuffers bytes ([`TypedProjectionData`]) carried in the snapshot frame's
/// `typed_projections` sidecar (ADR-0037). `nmp-core` never interprets those
/// bytes — the closure (owned by an app/protocol crate) encodes its own typed
/// schema and tags it with `schema_id` / `schema_version` / `file_identifier`.
///
/// Returns `None` when the projection has no changed payload this tick. Under
/// incremental apply, omission means the host retains its cached value; it is
/// never a clear signal. To clear a registered typed key, remove it through
/// [`SnapshotRegistry::remove`], which emits a one-shot `Cleared` row.
///
/// `Send + Sync` because the box lives behind an `Arc<Mutex<…>>` shared with
/// the actor thread (D8: the closure itself must also be non-blocking — it runs
/// inside the snapshot tick, exactly like a generic projection).
pub type TypedProjectionFn =
    Box<dyn Fn(u64) -> Option<TypedProjectionData> + Send + Sync + 'static>;

/// A feed-author-set provider (ADR-0063 D7, #1671 Lane H).
///
/// Returns the set of raw author keys a feed projection will RENDER for its
/// CURRENT visible window — recomputed fresh each time the kernel calls it. The
/// kernel reconciles this set against the prior tick's set for the same consumer
/// and auto-`resolve_ref`s the additions / `release_ref`s the removals, so a
/// shell cannot silently render an author it never resolved.
///
/// `Send + Sync` because the box lives behind the shared registry slot
/// (`Arc<Mutex<…>>`). D8: the kernel invokes it INSIDE the snapshot tick (so the
/// auto-resolve lands in the SAME frame the row appears — no 1-frame blank gap),
/// so it MUST be non-blocking — it only reads the engine's current window
/// (`snapshot_current_window`) and returns the keys; it does no I/O and waits on
/// no lock the actor thread could be holding.
pub type FeedAuthorProviderFn = Box<dyn Fn() -> Vec<String> + Send + Sync + 'static>;

// D5 — registration-count ceilings and the loud-no-op admission helper.
// Extracted to a `pub` submodule so the registry file stays within its LOC
// ceiling; the constant is part of the public D5 contract.
pub mod bounds;
use bounds::admit_keyed;

/// Result of a [`SnapshotRegistry::register_typed`] call (Blocker C).
///
/// Callers that record a composition-ledger disposition MUST derive it from
/// this result rather than from a pre-insertion key-presence check, so the
/// ledger stays truthful when the registry is at the D5 cap and silently drops
/// a new-key registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedAdmission {
    /// A new key was accepted and the closure inserted.
    Inserted,
    /// An existing key was replaced (always allowed regardless of the cap).
    Replaced,
    /// A new key was rejected because the registry is at the
    /// [`bounds::MAX_SNAPSHOT_PROJECTIONS`] ceiling (D5 loud no-op).
    DroppedFull,
}

// ADR-0053 — the host-declared consumed-projection set. Extracted to a `pub`
// submodule so the registry file stays within its LOC ceiling; the type is part
// of the public seam (read by the kernel to gate Tier-2 built-ins).
pub mod declared;
pub use declared::DeclaredProjections;

// ADR-0053 — end-to-end gating proofs. Mounted here (not from `kernel/mod.rs`)
// via `#[path]` so the kernel god-module stays at its size baseline. The test
// file uses absolute `crate::kernel::` paths so the mount point is irrelevant.
#[cfg(test)]
#[path = "declared_projections_tests.rs"]
mod declared_projections_tests;

// D5 — registration-count ceiling tests (kept beside the registry, off the
// `kernel/mod.rs` module list, so this PR does not touch that ratcheted file).
#[cfg(test)]
mod bounds_tests;

/// Registry of host-supplied snapshot projections.
///
/// Keyed by `String` so re-registering the same key replaces the old closure
/// rather than appending a duplicate. This prevents CPU waste: a re-registered
/// projection previously caused both the old and new closures to run on every
/// snapshot tick, with only the last result surfacing in the output.
#[derive(Default)]
pub struct SnapshotRegistry {
    typed_projections: HashMap<String, TypedProjectionFn>,
    /// One-shot `Cleared` rows for host-registered typed projections removed from
    /// the registry. Tier-1 keys are not in the built-in manifest, so Rung 3
    /// cannot synthesize these clears from `ProjectionPresence`.
    pending_typed_clears: Vec<String>,
    /// ADR-0063 D7 (#1671 Lane H) — feed-author-set providers, keyed by the feed
    /// snapshot key (e.g. `"nmp.feed.home"`) so a re-registration replaces (not
    /// duplicates) the provider and an `unregister_feed` removes it. Each closure
    /// returns the author keys its feed will RENDER this tick; the kernel
    /// reconciles them through `resolve_ref` inside the snapshot tick. Keyed by
    /// the feed key (not the derived consumer id) so the lifecycle matches the
    /// `typed_projections` registry exactly.
    feed_author_providers: HashMap<String, FeedAuthorProviderFn>,
    /// ADR-0053 — the host-declared set of consumed Tier-2 built-in projection
    /// keys. Empty (the default) means "no opinion / no narrowing" — every
    /// Tier-2 built-in is emitted, as before this ADR. A non-empty set narrows
    /// the kernel-owned built-ins to its members. Tier-1 host/protocol
    /// projections are unaffected (they self-gate by registration). See
    /// [`DeclaredProjections`].
    declared_projections: DeclaredProjections,
    /// ADR-0055 Rung 3 — the host-declared incremental-apply capability.
    ///
    /// `false` (the default) means "full rows every tick" — the kernel emits
    /// the complete typed sidecar on every `make_update`, unchanged from Rung 2.
    ///
    /// `true` means the host runtime owns the NMP cache-merge layer (D3-3) and
    /// the kernel is permitted to omit `Unchanged` projections from the frame.
    /// The host MUST set this before `nmp_app_start` (single-writer,
    /// set-before-start) via [`declare_incremental_apply`]. Durable architecture
    /// (per-attach baseline gate + Rung-5 ADR-0053 compose seam), NOT a compat
    /// shim — deleted only when every NMP host advertises it unconditionally.
    ///
    /// **Single source of truth (R6-S1).** This `Arc<AtomicBool>` is THE flag;
    /// `make_update` reads it lock-free and a Tier-1 producer captures a clone
    /// via [`Self::incremental_apply_handle`] — same atomic, no mirror anywhere.
    incremental_apply_enabled: Arc<AtomicBool>,
    /// ADR-0055 Rung 3 (D3-5) — one-shot latch set by `declare_incremental_apply`.
    ///
    /// The kernel reads and clears this in `make_update` (via
    /// `take_incremental_apply_baseline_pending`) and calls
    /// `ProjectionRevTracker::reset_last_emitted` when `true`, guaranteeing
    /// the next frame is a full baseline for the newly-declared host.
    incremental_apply_baseline_pending: bool,
    /// ADR-0055 R6-S1 — frame identity for Tier-1 producers that omit unchanged
    /// frames. `session_id` = `started_unix_ms` (changes on `Reset`-rebuild);
    /// `snapshot_epoch` = `tracker.epoch`. Written each tick by
    /// `Kernel::publish_frame_identity` before the typed projections run; a
    /// producer captures clones via [`Self::frame_identity_handles`] and
    /// rebaselines when EITHER differs from the last tuple — the SAME signal the
    /// host `ProjectionCache` resets on (the freeze fix). Lives here because the
    /// registry survives `Reset`.
    frame_session_id: Arc<AtomicU64>,
    frame_snapshot_epoch: Arc<AtomicU64>,
    /// ADR-0063 D7 (#1671 Lane H) — a monotone per-tick rev published at the TOP
    /// of every `make_update`, BEFORE any feed-author provider or typed producer
    /// closure runs.
    ///
    /// A [`FeedRenderSource`](nmp_feed::FeedRenderSource) keys its per-tick window
    /// memo on this value so the author provider (run first) and the typed
    /// producer (run later in the SAME tick) materialize the window EXACTLY ONCE
    /// and share it — closing the `load_older` 1-frame gap. Unlike
    /// `frame_snapshot_epoch` (which only changes on account-switch / schema bump)
    /// this changes on EVERY tick, so a feed re-materializes once per tick.
    frame_tick_rev: Arc<AtomicU64>,
    /// ADR-0063 D7 (#1671 Lane H) — the emitted-author sink for the structural
    /// guardrail (BLOCKING 2).
    ///
    /// Each feed's typed producer, when it materializes the window it ENCODES onto
    /// the wire this tick, records that window's actual author keys here under its
    /// `feed-author:<feed_key>` consumer id. The kernel reads this AFTER the typed
    /// projections are emitted and warns (debug-only) for any EMITTED author with
    /// no live resolver demand — catching a missed provider OR a `FeedAuthorRefs`
    /// field the provider's author set didn't cover (the row crossed the wire but
    /// was never resolved). Cleared each tick when the rev advances so a stale
    /// feed's authors don't linger. `BTreeSet` for dedup; keyed by consumer id.
    ///
    /// An `Arc<Mutex<…>>` (NOT a plain field) because a typed-producer closure
    /// writes to it WHILE the registry's own mutex is held by `run_typed()` — it
    /// captures a clone of THIS handle (via [`Self::emitted_feed_authors_handle`])
    /// and writes without re-locking the registry (which would deadlock).
    emitted_feed_authors: EmittedFeedAuthorsSlot,
}

/// ADR-0063 D7 (#1671 Lane H) — the shared emitted-author sink handle: the tick
/// rev it was last written for, and `consumer_id → emitted author keys`.
pub type EmittedFeedAuthorsSlot =
    Arc<Mutex<(u64, HashMap<String, std::collections::BTreeSet<String>>)>>;

/// ADR-0063 D7 (#1671 Lane H) — record `authors` as EMITTED under `consumer_id`
/// for `tick_rev` into the shared sink, clearing the sink when the rev advances.
///
/// A free function (not a method) so a typed-producer closure that captured a
/// clone of the [`EmittedFeedAuthorsSlot`] handle can write WITHOUT holding a
/// `&SnapshotRegistry` (it runs inside `run_typed()` while the registry mutex is
/// already held). A poisoned sink mutex (D6) is a silent no-op.
pub fn record_emitted_feed_authors(
    slot: &EmittedFeedAuthorsSlot,
    tick_rev: u64,
    consumer_id: impl Into<String>,
    authors: impl IntoIterator<Item = String>,
) {
    if let Ok(mut guard) = slot.lock() {
        if guard.0 != tick_rev {
            guard.0 = tick_rev;
            guard.1.clear();
        }
        let set = guard.1.entry(consumer_id.into()).or_default();
        set.extend(authors.into_iter().filter(|k| !k.is_empty()));
    }
}

use std::collections::HashMap;

impl SnapshotRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the projection(s) registered under `key` from the typed registry.
    ///
    /// Used by transient feeds (a visited profile / open thread) whose
    /// snapshot key must not outlive the screen: without this, the
    /// `register_feed`-installed closure keeps running on every 4 Hz tick and
    /// emits an empty subtree under a stale key forever (a leak — both wasted
    /// CPU and a phantom key in every `KernelSnapshot`). Returns
    /// `true` when the typed map held the key. Absent keys are a no-op.
    pub fn remove(&mut self, key: &str) -> bool {
        let removed_typed = self.typed_projections.remove(key).is_some();
        let clear_pending = self
            .pending_typed_clears
            .iter()
            .any(|pending| pending == key);
        if removed_typed && !clear_pending {
            self.pending_typed_clears.push(key.to_string());
        }
        removed_typed
    }

    /// Return the set of typed projection keys currently registered in the
    /// registry — without running any closures.
    ///
    /// Intended for coverage-gate tests that need to verify that a key was
    /// registered (regardless of whether its closure would return `Some` for
    /// the current state). Production code should use [`Self::run_typed`].
    #[must_use]
    pub fn registered_typed_keys(&self) -> impl Iterator<Item = &str> {
        self.typed_projections.keys().map(|k| k.as_str())
    }

    /// Register a **typed** projection closure under `key` — the
    /// FlatBuffers-sidecar seam (ADR-0037).
    ///
    /// `key` is the host-chosen snapshot namespace (e.g. `"nmp.feed.home"`).
    /// Registering the same key twice replaces the first — last-writer-wins, with
    /// no duplicate-closure CPU cost on subsequent ticks.
    ///
    /// D5: same [`MAX_SNAPSHOT_PROJECTIONS`](bounds::MAX_SNAPSHOT_PROJECTIONS) ceiling;
    /// re-registering an existing key is always allowed.
    ///
    /// Returns a [`TypedAdmission`] describing what actually happened so the caller
    /// can record a truthful composition-ledger disposition (Blocker C):
    /// - [`TypedAdmission::Inserted`] — new key accepted (was absent, below the cap).
    /// - [`TypedAdmission::Replaced`] — existing key replaced (always allowed).
    /// - [`TypedAdmission::DroppedFull`] — new key rejected (registry is at the
    ///   [`MAX_SNAPSHOT_PROJECTIONS`](bounds::MAX_SNAPSHOT_PROJECTIONS) ceiling).
    pub fn register_typed(
        &mut self,
        key: impl Into<String>,
        f: impl Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    ) -> TypedAdmission {
        self.register_typed_with_time(key, move |_| f())
    }

    /// Register a typed projection closure that receives the kernel-authored
    /// Unix timestamp for the snapshot tick.
    ///
    /// This is the narrow variant for protocol projections that render
    /// time-derived fields (for example age/staleness) from kernel-authored
    /// time. The closure must still be read-only: state transitions belong in
    /// actor commands or explicit event observers.
    pub fn register_typed_with_time(
        &mut self,
        key: impl Into<String>,
        f: impl Fn(u64) -> Option<TypedProjectionData> + Send + Sync + 'static,
    ) -> TypedAdmission {
        let key = key.into();
        let key_exists = self.typed_projections.contains_key(&key);
        if !admit_keyed(
            self.typed_projections.len(),
            key_exists,
            &key,
            "typed snapshot projection",
        ) {
            return TypedAdmission::DroppedFull;
        }
        self.pending_typed_clears.retain(|pending| pending != &key);
        self.typed_projections.insert(key, Box::new(f));
        if key_exists {
            TypedAdmission::Replaced
        } else {
            TypedAdmission::Inserted
        }
    }

    /// Run every registered typed projection and collect the results into the
    /// vector that becomes the snapshot frame's `typed_projections` sidecar.
    ///
    /// Mirrors the removed generic `run`: each closure runs on the actor thread inside
    /// `make_update`, so it must be non-blocking (D8). `None` means retain the
    /// prior value; removed keys emit one pending `Cleared` row. Closure panics
    /// are swallowed inside [`catch_unwind`] (D6).
    pub fn run_typed(&mut self) -> Vec<TypedProjectionData> {
        self.run_typed_at(0)
    }

    /// Run every registered typed projection using the supplied kernel-authored
    /// Unix timestamp for time-aware producers.
    pub fn run_typed_at(&mut self, now_secs: u64) -> Vec<TypedProjectionData> {
        let mut out = Vec::with_capacity(self.typed_projections.len());
        for projection in self.typed_projections.values() {
            // `AssertUnwindSafe`: a boxed `Fn` closure is not `UnwindSafe`, but
            // a panic here is fully contained — nothing the closure touched is
            // observed again after it unwinds, so there is no broken-invariant
            // hazard. The default panic hook still prints the payload, so the
            // bug stays visible.
            match catch_unwind(AssertUnwindSafe(|| projection(now_secs))) {
                Ok(Some(data)) => out.push(data),
                // `Ok(None)`: nothing to emit this tick. `Err(_)`: the closure
                // panicked — swallow it (the namespace is omitted, the same
                // shape as an unregistered projection).
                Ok(None) | Err(_) => continue,
            }
        }
        out.extend(
            std::mem::take(&mut self.pending_typed_clears)
                .into_iter()
                .map(|key| TypedProjectionData {
                    key,
                    state: WireProjectionState::Cleared,
                    ..Default::default()
                }),
        );
        out
    }

    // ADR-0053 host-declared consumed-projection methods
    // (`declare_consumed_projections`, `declared_projections`) and the
    // Workstream-E3 declared ⊆ decodable drift gate live in the `declared`
    // submodule alongside `DeclaredProjections`.

    // ADR-0063 D7 (#1671 Lane H) — the feed-author-provider + emitted-author-sink
    // methods (`register_feed_author_provider`, `remove_feed_author_provider`,
    // `registered_feed_author_provider_keys`, `run_feed_author_provider(s)`,
    // `record_emitted_feed_authors`, `emitted_feed_authors_handle`,
    // `emitted_feed_authors_for_tick`) live in the `feed_authors` submodule to
    // keep this file under its 500-LOC hard ceiling. They operate on the
    // `feed_author_providers` / `emitted_feed_authors` fields defined above.
}

/// Shared snapshot-projection registry handle.
///
/// One `Arc` clone lives on `NmpApp` (`nmp-native-runtime`); another is
/// threaded to the actor thread and bound onto the kernel via
/// [`Kernel::set_snapshot_projection_handle`]. Registrations made through the
/// `NmpApp` clone are visible to the kernel without crossing the FFI boundary
/// on each tick — the same shared-`Arc` pattern as the observed-projection sink
/// slot.
pub type SnapshotProjectionSlot = Arc<Mutex<SnapshotRegistry>>;

/// Construct a fresh, empty [`SnapshotProjectionSlot`].
#[must_use]
pub fn new_snapshot_projection_slot() -> SnapshotProjectionSlot {
    Arc::new(Mutex::new(SnapshotRegistry::new()))
}

// Kernel-side accessors over the shared slot (set/take handle, run typed
// projections and ADR-0053 declared-set snapshot) live in
// the `kernel_access` submodule to keep this file within its LOC ceiling.
mod kernel_access;

// ADR-0055 Rung 3 — the `declare_incremental_apply` / `is_incremental_apply_enabled`
// / `take_incremental_apply_baseline_pending` inherent methods live in the
// `incremental_apply` submodule to keep this file within its LOC ceiling. The
// two backing fields remain on the struct definition above.
mod incremental_apply;

// ADR-0063 D7 (#1671 Lane H) — the feed-author-provider + emitted-author-sink
// inherent methods live in the `feed_authors` submodule to keep this file under
// its 500-LOC hard ceiling. The two backing fields remain on the struct above.
mod feed_authors;
