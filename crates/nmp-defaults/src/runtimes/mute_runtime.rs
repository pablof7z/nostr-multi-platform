//! NIP-51 mute-list runtime — wires the kind:10000 [`MuteListProjection`] into
//! an [`AppHost`] (observer + generic JSON projection + typed `NMUT` sidecar +
//! per-tick interest reconciler via [`MuteRuntimeController`]).
//!
//! Extracted from `runtimes.rs` to hold that module under the 500-LOC hard
//! ceiling (AGENTS.md file-size rule: extract, never bump the baseline). The
//! `register_mute_runtime` entry point is re-exported from `runtimes` so the
//! existing `runtimes::register_mute_runtime` / `nmp_defaults::register_mute_runtime`
//! paths are unchanged.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::{EventObserverRegistrar, HostCapabilities, SnapshotProjectionRegistrar};
use nmp_core::actor::ActorCommand;
use nmp_core::{KernelEventObserver};
use nmp_nip51::{active_mute_list_interest, active_mute_list_interest_id, MuteListProjection};

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
/// 3. **Snapshot projection (typed)** — registers a typed FlatBuffers sidecar
///    (ADR-0037, `NMUT`) under the `"nmp.nip51.mute_list"` key. Reads the same
///    `MuteListSnapshot` read model so it cannot structurally diverge from the
///    projection.
/// 4. **Tick observer — [`MuteRuntimeController`]** — registered LAST
///    (ordering contract: observer BEFORE tick observer). On every snapshot tick
///    reconciles the active pubkey against the last-pushed one, emitting
///    `PushInterest` / `WithdrawInterest` to the kernel so the mute list
///    interest (kind:10000, authors=[active]) is always live for the signed-in
///    account. This replaces the prior free-ride on `SELF_KINDS_TAILING`.
/// 5. **Returns the `Arc<MuteListProjection>`** — the caller wires
///    `set_suppression` on whichever `ModularTimelineProjection` it owns.
///
/// # Ordering contract
///
/// The event observer MUST be registered before the tick observer. The tick
/// observer pushes the mute-list interest on its first call, which triggers a
/// synchronous cache-serve drain. If the event observer is not registered yet at
/// that point, the drain delivers events to nobody. Register in this order:
/// 1. `app.register_event_observer(...)` — FIRST
/// 2. `app.register_typed_snapshot_projection(...)` — second
/// 3. `app.register_snapshot_tick_observer(...)` — LAST
///
/// # Account-switch safety
///
/// `MuteListProjection` is self-contained: the read path re-reads the live
/// `active_pubkey` slot on every call and gates against the `owner_pubkey`
/// stored inside the `MuteSet`. If the active account changed between the last
/// kind:10000 ingest and the read, the methods return `false` — stale data from
/// the prior account is invisible. `MuteRuntimeController` additionally withdraws
/// the prior interest and pushes a fresh one on account switch so no stale
/// subscription persists in the planner.
///
/// # D0 hygiene
///
/// This function names `kind:10000` only as a numeric literal inside nmp-nip51.
/// The term "mute" enters `nmp-core` nowhere: `SuppressionLookup` is the
/// substrate-generic trait; `"nmp.nip51.mute_list"` is a projection key string
/// owned by this composition crate. `nmp-core` sees `KernelEventObserver` +
/// `SuppressionLookup` only.
///
/// Called by [`crate::register_defaults`]; exposed `pub` so an app crate that
/// opts out of the wholesale defaults can still wire just the mute runtime by
/// itself.
pub fn register_mute_runtime(
    app: &(impl EventObserverRegistrar + HostCapabilities + SnapshotProjectionRegistrar),
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

    // ── 2. Register as ingest observer — FIRST (ordering contract) ──────────
    //
    // Must be registered BEFORE the tick observer below. The tick observer
    // pushes the mute-list interest on its first call, which triggers a
    // synchronous cache-serve drain. If this observer is not registered yet at
    // that point, the drain delivers events to nobody.
    app.register_event_observer(Arc::clone(&mute) as Arc<dyn KernelEventObserver>);

    // ── 3. Snapshot projection (typed sidecar) ────────────────────────────────
    //
    // Typed FlatBuffers sidecar (ADR-0037, `NMUT`) registered under the
    // `"nmp.nip51.mute_list"` key. Reads the same `MuteListSnapshot` read model
    // so it cannot structurally diverge from the projection. Pure read — no
    // side effects (D0). The projection key carries no NIP-51 nouns through
    // `nmp-core` — the composition root (this crate) is entitled to name NIP
    // constants directly per ADR-0046.
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

    // ── 4. Per-tick reconciler — LAST (ordering contract) ────────────────────
    //
    // `MuteRuntimeController` owns the active-account kind:10000 interest slot.
    // On sign-in it pushes `active_mute_list_interest(pubkey)` so the kernel
    // has a live `authors=[active_pubkey] / kinds=[10000]` subscription. On
    // account switch it withdraws the old interest (by pubkey-invariant id) and
    // pushes a new one. On sign-out it withdraws. Mirrors the NIP-57 zap-receipts
    // controller (`ZapReceiptsRuntimeController`).
    let controller = Arc::new(MuteRuntimeController {
        active_pubkey: app.active_pubkey(),
        tx: app.actor_sender(),
        last_pushed_pubkey: Mutex::new(None),
    });
    let controller_tick = Arc::clone(&controller);
    app.register_snapshot_tick_observer(move || controller_tick.tick());

    mute
}

/// Per-tick reconciler for the active-account mute-list interest.
///
/// Owns the kind:10000 `authors=[active_pubkey]` interest slot. On every
/// snapshot tick diffs the active pubkey against the last-pushed one and
/// enqueues `PushInterest` / `WithdrawInterest` on change (D8: non-blocking).
///
/// Exposed `pub(crate)` so the unit tests in `runtimes_mute_tests` can
/// construct a controller without a real `AppHost`.
pub(crate) struct MuteRuntimeController {
    /// Pubkey-only identity slot (Finding C): the active account's hex pubkey,
    /// populated for every backend including bunker. Identity only — never
    /// secret key material.
    pub(crate) active_pubkey: nmp_core::slots::ActiveAccountSlot,
    pub(crate) tx: nmp_core::CommandSender,
    pub(crate) last_pushed_pubkey: Mutex<Option<String>>,
}

impl MuteRuntimeController {
    /// Reconcile the active-account mute-list interest once per snapshot tick.
    ///
    /// Diffs the active pubkey against the last-pushed one and enqueues
    /// `PushInterest` / `WithdrawInterest` on change. D8: channel send is
    /// non-blocking; D6: a poisoned last-pushed mutex degrades to "no prior
    /// push" so the next sign-in still pushes.
    pub(crate) fn tick(&self) {
        let active = self.active_pubkey();

        let mut last = self
            .last_pushed_pubkey
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match (active.as_deref(), last.as_deref()) {
            // No change — common case, fast path, no actor traffic.
            (Some(now), Some(prev)) if now == prev => {}
            // Sign-in (or first-ever push).
            (Some(now), None) => {
                let _ = self
                    .tx
                    .send(ActorCommand::PushInterest(active_mute_list_interest(now)));
                *last = Some(now.to_string());
            }
            // Account switch: withdraw old (by pubkey-invariant id), push new.
            (Some(now), Some(_prev)) => {
                let _ = self
                    .tx
                    .send(ActorCommand::WithdrawInterest(active_mute_list_interest_id()));
                let _ = self
                    .tx
                    .send(ActorCommand::PushInterest(active_mute_list_interest(now)));
                *last = Some(now.to_string());
            }
            // Logout: withdraw standing interest, clear slot.
            (None, Some(_)) => {
                let _ = self
                    .tx
                    .send(ActorCommand::WithdrawInterest(active_mute_list_interest_id()));
                *last = None;
            }
            // Cold start before sign-in: nothing to do.
            (None, None) => {}
        }
    }

    fn active_pubkey(&self) -> Option<String> {
        self.active_pubkey
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }
}
