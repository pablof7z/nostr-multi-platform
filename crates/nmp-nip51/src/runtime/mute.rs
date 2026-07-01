//! NIP-51 mute-list runtime — wires the kind:10000 [`MuteListProjection`] into
//! a host (active observed projection + typed `NMUT` sidecar +
//! identity-change reconciler).
//!
//! This is the owner-local runtime API for the public mute-list projection.

use std::sync::Arc;

use crate::{
    active_mute_list_interest, encode_mute_list, MuteListProjection, MUTE_LIST_FILE_IDENTIFIER,
    MUTE_LIST_SCHEMA_ID, MUTE_LIST_SCHEMA_VERSION,
};
use nmp_core::substrate::{
    HostCapabilities, IdentityChangeRegistrar, ObservedProjectionReconciler,
    ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;

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
/// 2. **Active observed projection** — opens `MuteListProjection` through an
///    `authors=[active] / kinds=[10000]` observed projection only after an
///    active account exists. This replays matching cached rows before live
///    activation without opening a broad kind-only observer.
/// 3. **Snapshot projection (typed)** — registers a typed FlatBuffers sidecar
///    (ADR-0037, `NMUT`) under the `"nmp.nip51.mute_list"` key. Reads the same
///    `MuteListSnapshot` read model so it cannot structurally diverge from the
///    projection.
/// 4. **Identity-change observer** — registered LAST. On every account change
///    reconciles the active pubkey against the currently opened observed
///    projection, closing the old author shape and opening the new one as
///    needed. An eager `sync()` after wiring covers cold-start (account may
///    already be set before this registration).
/// 5. **Returns the `Arc<MuteListProjection>`** — the caller wires
///    `set_suppression` on whichever `ModularTimelineProjection` it owns.
///
/// # Ordering contract
///
/// The observed projection MUST NOT open until the active pubkey is known.
/// Opening with `authors=[active]` uses the ADR-0062 replay path, so cold-start
/// cache rows hydrate the projection before activation.
///
/// # Account-switch safety
///
/// `MuteListProjection` is self-contained: the read path re-reads the live
/// `active_pubkey` slot on every call and gates against the `owner_pubkey`
/// stored inside the `MuteSet`. If the active account changed between the last
/// kind:10000 ingest and the read, the methods return `false` — stale data from
/// the prior account is invisible. The active observed-projection reconciler
/// additionally closes the prior author shape and opens the new one on account
/// switch so no stale subscription persists in the planner.
///
/// # D0 hygiene
///
/// This function names `kind:10000` only as a numeric literal inside nmp-nip51.
/// The term "mute" enters `nmp-core` nowhere: `SuppressionLookup` is the
/// substrate-generic trait; `"nmp.nip51.mute_list"` is a projection key string
/// owned by this composition crate. `nmp-core` sees `ObservedProjectionSink` +
/// `SuppressionLookup` only.
///
/// Exposed as a named per-feature installer so app composition roots can wire
/// mute support without pulling in any defaults bundle.
pub fn register_mute_runtime(
    app: &(impl ObservedProjectionRegistrar
          + HostCapabilities
          + SnapshotProjectionRegistrar
          + IdentityChangeRegistrar),
) -> Arc<MuteListProjection> {
    // ── 1. Active-pubkey slot ────────────────────────────────────────────────
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

    // ── 2. Snapshot projection (typed sidecar) ────────────────────────────────
    //
    // Typed FlatBuffers sidecar (ADR-0037, `NMUT`) registered under the
    // `"nmp.nip51.mute_list"` key. Reads the same `MuteListSnapshot` read model
    // so it cannot structurally diverge from the projection. Pure read — no
    // side effects (D0). The projection key carries no NIP-51 nouns through
    // `nmp-core` — the composition root (this crate) is entitled to name NIP
    // constants directly per ADR-0046.
    let mute_for_typed = Arc::clone(&mute);
    app.register_typed_snapshot_projection(
        nmp_ownership::DeclaredProjectionKey::framework(
            "nmp.nip51.mute_list",
            "projection.nmp.nip51.mute_list",
        ),
        move || {
            let snapshot = mute_for_typed.snapshot();
            Some(nmp_core::TypedProjectionData {
                key: "nmp.nip51.mute_list".to_string(),
                schema_id: MUTE_LIST_SCHEMA_ID.to_string(),
                schema_version: MUTE_LIST_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(MUTE_LIST_FILE_IDENTIFIER).into_owned(),
                payload: encode_mute_list(&snapshot),
                ..Default::default()
            })
        },
    );

    // ── 3. Account-change reader notification ───────────────────────────────
    let mute_for_identity = Arc::clone(&mute);
    app.register_identity_change_observer(move |_| mute_for_identity.notify_account_changed());

    // ── 4. Active observed-projection reconciler — LAST ─────────────────────
    //
    // Identity-change-driven: no tick polling. The live_shape closure reads the
    // active pubkey slot directly, returning Some(shape) when signed in and
    // None on logout/reset so no stale subscription lingers.
    let observer = Arc::clone(&mute) as Arc<dyn ObservedProjectionSink>;
    let active_pubkey = app.active_pubkey();
    let reconciler = ObservedProjectionReconciler::new(
        app.observed_projection_registrar_handle(),
        observer,
        "nmp.nip51.mute_list",
        1,
        128,
        Arc::new(move || {
            let pubkey = active_pubkey.lock().ok()?.clone()?;
            Some(active_mute_list_interest(&pubkey).shape)
        }),
    );
    let reconciler_for_identity = reconciler.clone();
    app.register_identity_change_observer(move |_| reconciler_for_identity.sync());
    // Eager sync for cold-start: account may already be set.
    reconciler.sync();

    mute
}

#[cfg(test)]
#[path = "mute_tests.rs"]
mod tests;
