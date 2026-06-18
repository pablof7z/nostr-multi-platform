//! NIP-51 mute-list runtime — wires the kind:10000 [`MuteListProjection`] into
//! an [`AppHost`] (observer + generic JSON projection + typed `NMUT` sidecar).
//!
//! Extracted from `runtimes.rs` to hold that module under the 500-LOC hard
//! ceiling (AGENTS.md file-size rule: extract, never bump the baseline). The
//! `register_mute_runtime` entry point is re-exported from `runtimes` so the
//! existing `runtimes::register_mute_runtime` / `nmp_defaults::register_mute_runtime`
//! paths are unchanged.

use std::sync::Arc;

use nmp_core::substrate::{EventObserverRegistrar, HostCapabilities, SnapshotProjectionRegistrar};
use nmp_core::KernelEventObserver;
use nmp_nip51::MuteListProjection;

/// Wire the NIP-51 mute-list observer into `app` and return the
/// [`MuteListProjection`] so the caller can connect it to a timeline
/// projection via [`nmp_nip01::ModularTimelineProjection::set_suppression`].
///
/// # What this function does
///
/// 1. **Pubkey slot bridge** — hands [`MuteListProjection`] the shared
///    `AppHost::active_pubkey()` hex slot (populated by the kernel for EVERY
///    backend including bunker). The projection reads it at event-ingest time
///    and at query time, so it is always consistent with the active account.
/// 2. **Ingest observer** — registers `MuteListProjection` as a
///    [`KernelEventObserver`] so the kernel fan-out delivers kind:10000 events.
///    The projection filters for the active account's author and ignores all
///    other kind:10000 events (account-switch safety is enforced at read time by
///    the owner-pubkey gate inside the projection — no explicit reset needed).
/// 3. **Snapshot projection (generic + typed)** — registers
///    `"nmp.nip51.mute_list"` as a generic JSON projection AND a typed
///    FlatBuffers sidecar (ADR-0037, `NMUT`) under the same key. Both read the
///    same `MuteListSnapshot` read model so they cannot structurally diverge.
///    The projection key carries no NIP-51 nouns through `nmp-core` — the
///    composition root (this crate) is entitled to name NIP constants directly
///    per ADR-0046.
/// 4. **Returns the `Arc<MuteListProjection>`** — the caller wires
///    `set_suppression` on whichever `ModularTimelineProjection` it owns. The
///    split between observer registration (here) and suppression wiring
///    (caller-side) is intentional: `AppHost` has no `set_suppression` seam, and
///    nmp-defaults must not depend on any concrete timeline instance (it is
///    composition-root neutral).
///
/// # Account-switch safety
///
/// `MuteListProjection` is self-contained: the read path (`is_suppressed_author`,
/// `is_suppressed_event`) re-reads the live `active_pubkey` slot on every call and
/// gates against the `owner_pubkey` stored inside the `MuteSet`. If the active
/// account changed between the last kind:10000 ingest and the read, the methods
/// return `false` — stale data from the prior account is invisible. No explicit
/// reset call or identity-change observer is required. This is the same read-time
/// owner-gate pattern documented in `nmp-nip51/src/projection.rs`.
///
/// # D0 hygiene
///
/// This function names `kind:10000` only as a numeric literal. The term "mute"
/// enters `nmp-core` nowhere: `SuppressionLookup` is the substrate-generic trait;
/// `"nmp.nip51.mute_list"` is a projection key string owned by this composition
/// crate. `nmp-core` sees `KernelEventObserver` + `SuppressionLookup` only.
///
/// Called by [`crate::register_defaults`]; exposed `pub` so an app crate that
/// opts out of the wholesale defaults can still wire just the mute runtime by
/// itself.
///
/// # Ordering
///
/// Like [`crate::register_defaults`], call before `nmp_app_start`. The
/// `KernelEventObserver` must be registered before the first event arrives.
pub fn register_mute_runtime(
    app: &(impl EventObserverRegistrar + HostCapabilities + SnapshotProjectionRegistrar),
) -> Arc<MuteListProjection> {
    // ── 1. Active-pubkey slot (Finding C) ────────────────────────────────
    //
    // `MuteListProjection` takes `Arc<Mutex<Option<String>>>` (hex pubkey) —
    // exactly the shape of `AppHost::active_pubkey()` (`ActiveAccountSlot`),
    // populated by the kernel for EVERY backend including bunker. We hand the
    // projection that shared slot directly: no keys→hex bridge tick observer
    // (the old code derived hex from `active_local_keys()`, silently dead for
    // bunker accounts), and a single source of truth for the active pubkey (D4)
    // rather than a second mirrored hex slot. The projection reads the slot at
    // ingest AND `is_suppressed_*` query time, so both see the live account.
    let mute = Arc::new(MuteListProjection::new(app.active_pubkey()));

    // ── 2. Register as ingest observer ───────────────────────────────────
    //
    // The WOT bootstrap interest already includes kind:10000 in its
    // `WOT_BOOTSTRAP_KINDS` filter (see `nmp-wot/src/interest.rs`), so the
    // active account's kind:10000 will arrive via the existing subscription.
    // No separate interest push is needed.
    app.register_event_observer(Arc::clone(&mute) as Arc<dyn KernelEventObserver>);

    // ── 3. Snapshot projection (generic + typed sidecar) ─────────────────
    //
    // Emits `{"muted_pubkeys":[…],"muted_event_ids":[…]}` on every tick so the
    // active account's mute list is visible in the kernel snapshot.
    //
    // Typed FlatBuffers sidecar (ADR-0037, `NMUT`) registered ALONGSIDE the
    // generic `Value` projection under the same key (additive — a `NMUT`-aware
    // host prefers it, others fall back). Both wire forms read the SAME
    // `MuteListSnapshot` read model so they cannot structurally diverge; the
    // typed closure is a pure read (the projection holds no per-tick
    // book-keeping). Clone the `Arc` first: the generic closure below consumes
    // its clone.
    let mute_for_typed = Arc::clone(&mute);
    app.register_typed_snapshot_projection("nmp.nip51.mute_list", move || {
        let snapshot = mute_for_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip51.mute_list".to_string(),
            schema_id: nmp_nip51::MUTE_LIST_SCHEMA_ID.to_string(),
            schema_version: nmp_nip51::MUTE_LIST_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(nmp_nip51::MUTE_LIST_FILE_IDENTIFIER)
                .into_owned(),
            payload: nmp_nip51::encode_mute_list(&snapshot),
            ..Default::default()
        })
    });

    mute
}
