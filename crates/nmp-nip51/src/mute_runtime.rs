//! NIP-51 mute-list runtime — wires the kind:10000 [`MuteListProjection`] into
//! an [`AppHost`] (active observed projection + typed `NMUT` sidecar +
//! identity-change reconciler).

use std::sync::Arc;

use nmp_core::substrate::{
    HostCapabilities, IdentityChangeRegistrar, ObservedProjectionReconciler,
    ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;

use crate::{active_mute_list_interest, MuteListProjection};

const MUTE_LIST_PROJECTION_KEY: nmp_ownership::DeclaredProjectionKey =
    nmp_ownership::DeclaredProjectionKey::framework(
        "nmp.nip51.mute_list",
        "projection.nmp.nip51.mute_list",
    );

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
/// substrate-generic trait; `"nmp.nip51.mute_list"` is a projection key string.
/// `nmp-core` sees `ObservedProjectionSink` + `SuppressionLookup` only.
pub fn register_mute_runtime(
    app: &(impl ObservedProjectionRegistrar
          + HostCapabilities
          + SnapshotProjectionRegistrar
          + IdentityChangeRegistrar),
) -> Arc<MuteListProjection> {
    // ── 1. Active-pubkey slot ────────────────────────────────────────────────
    let mute = Arc::new(MuteListProjection::new(app.active_pubkey()));

    // ── 2. Snapshot projection (typed sidecar) ────────────────────────────────
    let mute_for_typed = Arc::clone(&mute);
    app.register_typed_snapshot_projection(MUTE_LIST_PROJECTION_KEY, move || {
        let snapshot = mute_for_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip51.mute_list".to_string(),
            schema_id: crate::MUTE_LIST_SCHEMA_ID.to_string(),
            schema_version: crate::MUTE_LIST_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(crate::MUTE_LIST_FILE_IDENTIFIER).into_owned(),
            payload: crate::encode_mute_list(&snapshot),
            ..Default::default()
        })
    });

    // ── 3. Account-change reader notification ───────────────────────────────
    let mute_for_identity = Arc::clone(&mute);
    app.register_identity_change_observer(move |_| mute_for_identity.notify_account_changed());

    // ── 4. Active observed-projection reconciler — LAST ─────────────────────
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
