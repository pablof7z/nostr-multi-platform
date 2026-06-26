//! Public pure reducer over [`KernelAction`] → [`KernelUpdate`].
//!
//! `nmp-codegen` projects per-app FFI crates that own an `AppAction` /
//! `AppUpdate` pair around [`KernelAction`] / [`KernelUpdate`]. The generated
//! `FfiApp::dispatch` needs to reduce the kernel arm to an update — but the
//! [`crate::kernel_action::dispatch_kernel_action`] reducer (also used by the
//! actor loop) is `pub(crate)` and takes a private `&mut Kernel`, neither
//! reachable from a downstream crate.
//!
//! [`KernelReducer`] closes that seam: it owns an encapsulated [`Kernel`] and
//! exposes a single public method — [`KernelReducer::reduce`] — that delegates
//! to the same hand-written reducer the actor uses. [`KernelAction::OpenUri`]
//! registers a subscription interest through the kernel's single-writer
//! registry; unavailable lifecycle/view placeholders return explicit rejected
//! update data.
//!
//! # V-01 Stage 3 — relay-frame ingestion surface
//!
//! In addition to the [`KernelReducer::reduce`] action seam above, this type
//! exposes a small set of relay-lifecycle methods —
//! [`KernelReducer::handle_relay_frame`],
//! [`KernelReducer::handle_relay_connected`],
//! [`KernelReducer::handle_relay_failed`],
//! [`KernelReducer::handle_relay_closed`], and [`KernelReducer::tick`] —
//! that mirror the per-event arms the native `actor::dispatch::handle_relay_event`
//! handles for each `nmp_network::relay_worker::RelayEvent` variant. The wasm32
//! `BrowserRelayDriver` in `nmp-wasm` is callback-driven (no thread, no
//! blocking `read_frame`) so it cannot share the native `run_relay_worker`
//! loop; instead it owns the WebSocket lifecycle directly and feeds each
//! callback through these methods. The native actor still uses
//! [`crate::kernel::Kernel::handle_message`] directly through its private path;
//! the public methods here delegate to the **same** underlying methods, so
//! kernel behaviour is byte-for-byte identical across both transports.
//!
//! Doctrine:
//! - **D0** — the public surface deals only in app-noun-free primitives
//!   ([`RelayFrame`], [`OutboundMessage`], [`RelayRole`] are substrate types).
//! - **D6** — total function: never panics, never unwinds across FFI.
//!   Failures funnel into rejected update data.
//! - **D8** — runs once per *action* / *frame*, not in a poll loop.
//!
//! This is the NMP-145 follow-up: T-NMP-145-FF.

use crate::app::{KernelAction, KernelUpdate};
use crate::kernel::{Kernel, SnapshotProjectionSlot};
use crate::kernel_action::dispatch_kernel_action;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::collections::HashMap;

/// Encapsulated kernel + public pure reducer.
///
/// Owns the [`Kernel`] privately so codegen-driven `FfiApp`s can reduce
/// [`KernelAction`] values to [`KernelUpdate`] values without depending on
/// crate-internal types. Two shared slots (`observer_slot`, `snapshot_slot`)
/// support the PR-4 wasm32 composition seams in `composition_seams.rs`.
pub struct KernelReducer {
    kernel: Kernel,
    /// Headless event-observer slot (no drain thread — wasm32 safe).
    observer_slot: crate::actor::ObservedProjectionSinkSlot,
    /// Typed snapshot-projection slot.
    snapshot_slot: SnapshotProjectionSlot,
    /// Close recipes for browser/wasm observed-projection sessions.
    observed_projection_sessions:
        HashMap<crate::ObservedProjectionId, (String, String, u32, Option<String>)>,
    /// #1753 S6 — wasm signing round-trip state: the shared `ParkedSignerOps`
    /// queue + drain driver (the SAME component the native actor loop uses), the
    /// per-correlation value-delivery senders, the account-pin map, and the
    /// observable completion sink. Unused on every native path (the actor owns
    /// its own queue); populated only when a wasm host drives the
    /// `begin_sign_roundtrip` / `deliver_signed_response` seam. Definition +
    /// methods live in `kernel_reducer/wasm_signing.rs`.
    sign_roundtrip: wasm_signing::SignRoundTripState,
}

impl KernelReducer {
    /// Construct a fresh reducer with the default visible-limit. Equivalent
    /// to what the actor loop uses at startup.
    ///
    /// On all targets (including wasm32) this binds a headless
    /// [`ObservedProjectionSinkSlot`] and a [`SnapshotProjectionSlot`] into the
    /// kernel so that composition roots can register observed projections and
    /// typed projections without spawning background threads.
    #[must_use]
    pub fn new() -> Self {
        use crate::actor::new_event_observer_slot_headless;
        use crate::kernel::new_snapshot_projection_slot;
        use std::sync::Arc;

        let observer_slot = new_event_observer_slot_headless();
        let snapshot_slot = new_snapshot_projection_slot();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.set_event_observers_handle(Arc::clone(&observer_slot));
        kernel.set_snapshot_projection_handle(Arc::clone(&snapshot_slot));
        Self {
            kernel,
            observer_slot,
            snapshot_slot,
            observed_projection_sessions: HashMap::new(),
            sign_roundtrip: wasm_signing::SignRoundTripState::default(),
        }
    }

    /// Build a passive pre-start snapshot through the normal kernel snapshot
    /// encoder.
    ///
    /// Native FFI registers update callbacks before the actor exists. This
    /// helper gives that passive state a kernel-authored frame anyway: it binds
    /// the same queue-depth diagnostic handle the actor will later bind, applies
    /// an injected test/replay clock when present, and then delegates to
    /// [`Kernel::make_update`] through [`Self::make_update_frame`].
    pub fn passive_snapshot_frame(
        clock: Option<std::sync::Arc<dyn crate::Clock>>,
        queue_depth: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) -> crate::UpdateFrameBytes {
        let mut reducer = Self::new();
        if let Some(clock) = clock {
            reducer.kernel.set_clock(clock);
        }
        reducer.kernel.set_queue_depth_handle(queue_depth);
        reducer.make_update_frame(false)
    }

    /// Reduce one [`KernelAction`] against the encapsulated kernel, returning
    /// the [`KernelUpdate`] the host app should observe.
    ///
    /// Total and panic-free (D6): fallible actions and unavailable placeholders
    /// return explicit rejected update data.
    pub fn reduce(&mut self, action: KernelAction) -> KernelUpdate {
        dispatch_kernel_action(&mut self.kernel, action)
    }

    /// Current wall-clock milliseconds via the reducer-owned kernel clock.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        self.kernel.now_ms()
    }

    /// Current wall-clock seconds via the reducer-owned kernel clock.
    #[must_use]
    pub fn now_secs(&self) -> u64 {
        self.kernel.now_secs()
    }

    /// Clock injection for non-actor runtimes such as `nmp-wasm`.
    pub fn set_clock_for_test(&mut self, clock: std::sync::Arc<dyn crate::Clock>) {
        self.kernel.set_clock(clock);
    }

    /// Returns `true` when the kernel state has changed since the last
    /// `make_update_frame` call. The wasm32 event/deadline scheduler checks
    /// this before pushing a snapshot so idle deadline drains do not produce
    /// spurious frames (dirty-flag coalescing, PR-2 rider).
    pub fn changed_since_emit(&self) -> bool {
        self.kernel.changed_since_emit()
    }

    // ─── F-CR-00 component-owned claim seam ─────────────────────────────────
    //
    // Wasm consumers (chirp-web components) have no ActorCommand channel —
    // they drive the kernel through `KernelReducer` directly. These methods
    // expose the same `Kernel::resolve_ref` / `release_ref` surface the actor
    // uses on native, so web components can self-claim profiles and events on
    // mount/unmount the same way iOS (`chirp-avatar.<uuid>`) and Android
    // (`note-author-<eventId>`) do.
    //
    // Post-processing mirrors `publish_signed_event`: the outbound the kernel
    // returns is run through `partition_auth_paused` before delivery to the
    // caller, so a claim on a relay mid-NIP-42 handshake is buffered inside
    // the kernel and replayed on the next tick after `Authenticated` — identical
    // to the native `send_all_outbound` invariant.
    //
    // D6 — total: every kernel method is already total (malformed inputs return
    // `Vec::new()`; no panics); the thin delegations here add no new failure
    // paths.
    //
    // D8 — no polling. Claims are reactive dispatch; the kernel registers
    // interest and the wasm `dispatch()` arm fans the outbound immediately.

    // ADR-0063 Lane H: claim_profile / release_profile deleted.
    // Use resolve_profile_ref / release_profile_ref directly (or the
    // KernelReducer::resolve_ref / release_ref if a public surface is needed).

    /// `claim_send_gate` equivalent for the wasm dispatch path — returns
    /// `true` as soon as any relay lane has reported `Connected`.
    ///
    /// Mirrors `actor::relay_mgmt::claim_send_gate` (which reads a
    /// `HashSet<RelayRole>` the actor maintains). On the wasm path the
    /// kernel's per-lane `RelayHealth::connection` field is the authoritative
    /// signal: `handle_relay_connected` → `relay_connected_url` →
    /// `mark_lane_connected` sets it to `"connected"`. Using this accessor
    /// rather than driver-socket state (`current_socket.is_some()` fires at
    /// dial time, before `Connected`) avoids the lost-fetch trap.
    #[must_use]
    pub fn any_relay_connected(&self) -> bool {
        self.kernel.any_relay_connected()
    }

    /// Read the active-account pubkey the kernel currently holds (lowercase
    /// canonical hex), or `None` if no active account is set.
    ///
    /// Bounded reducer helper: wasm uses this to fail closed before building
    /// write/sign requests and tests use it to verify canonicalization. It is
    /// not a projection substitute and does not expose account lists or signer
    /// roster state.
    #[must_use]
    pub fn active_account_pubkey(&self) -> Option<String> {
        self.kernel.active_account_pubkey().map(|s| s.to_string())
    }

    /// V-51 phase 2 — render the kernel's routing-trace projection as JSON.
    ///
    /// The shape is documented at
    /// [`crate::kernel::routing_trace_dto`]: a `schema_version`-keyed object
    /// carrying `publishes` and `subscriptions` arrays with per-URL
    /// `lanes[]` attribution.
    ///
    /// Bounded diagnostics seam — the `nmp-wasm` runtime exposes this to JS
    /// hosts (`NmpWasmRuntime::recent_routing_decisions`) so the web Chirp
    /// shell can render the same routing inspector iOS gets through the
    /// unified debug-info FFI symbol. Native callers reach the same
    /// kernel-authored projection handle directly.
    ///
    /// D6 — total: the projection always exists (`Kernel::new` constructs
    /// it); a serialisation hiccup falls back to an empty-rings document.
    #[must_use]
    pub fn recent_routing_decisions_json(&self) -> String {
        let value = crate::projection_to_json(&self.kernel.routing_trace());
        serde_json::to_string(&value).unwrap_or_else(|_| {
            String::from(r#"{"schema_version":1,"capacity":0,"publishes":[],"subscriptions":[]}"#)
        })
    }

    /// Build one FlatBuffers update frame from the current kernel state.
    ///
    /// Forwards to [`crate::kernel::Kernel::make_update`] which bumps the
    /// kernel's monotonic revision, runs all typed projections (including
    /// the `configured_relays` and `relay_statuses` Tier-3 rows), drains
    /// `emit` observers, and encodes the complete Tier-3 + Tier-2 frame.
    /// The caller does **not** need to maintain a separate revision counter
    /// — the kernel is the sole owner of `rev` (D4).
    ///
    /// D6 — total: never panics; `make_update` is unconditional for any
    /// reducer that has been successfully constructed.
    pub fn make_update_frame(&mut self, running: bool) -> crate::UpdateFrameBytes {
        self.kernel.make_update(running)
    }
}

/// Test-support seam: fire the observer slot directly with a `KernelEvent`.
///
/// This is the substrate-clean path for wasm32 integration tests that cannot
/// go through `ingest_pre_verified_event` (a `pub(crate)` kernel method).
/// It mirrors exactly what `Kernel::notify_event_observers` does on the
/// production ingest path: snapshot observers under the lock and fire each
/// synchronously.
///
/// Only available under `cfg(any(test, feature = "test-support"))`. Never
/// call from production code — use `handle_relay_frame` for real ingest.
#[cfg(any(test, feature = "test-support"))]
impl KernelReducer {
    pub fn fire_event_observers_for_test(&self, event: &crate::substrate::KernelEvent) {
        crate::actor::notify_observers(&self.observer_slot, event);
    }
}

// #2045 PR-A — narrow headless command interpreter: `apply_actor_command` +
// `CommandApplyOutcome`. Shared by the wasm runtime and any future headless
// runtime so there is one command-application path.
mod command_apply;
pub use command_apply::CommandApplyOutcome;
mod composition_seams;
mod composition_seams_browser; // PR-B (#2046) AppHost seams factored out for LOC ceiling
mod feed_verbs;
mod follow;
mod react;
mod refs;
mod relay_lifecycle;
mod reply;
// #1753 S6 — the wasm signing capability round-trip seam (pure message
// re-entry). Adds `begin_sign_roundtrip` / `deliver_signed_response` to
// `impl KernelReducer` and defines `SignRoundTripState` + its public DTOs.
mod wasm_signing;
pub use wasm_signing::{SignRoundTripCompletion, SignRoundTripOutcome, SignRoundTripRequest};

impl Default for KernelReducer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "kernel_reducer/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "kernel_reducer/tests_snapshot_claims.rs"]
mod tests_snapshot_claims;

#[cfg(test)]
#[path = "kernel_reducer/tests_feed_verbs.rs"]
mod tests_feed_verbs;

#[cfg(test)]
#[path = "kernel_reducer/tests_reply_tags.rs"]
mod tests_reply_tags;

#[cfg(test)]
#[path = "kernel_reducer/tests_react.rs"]
mod tests_react;

#[cfg(test)]
#[path = "kernel_reducer/tests_follow.rs"]
mod tests_follow;

#[cfg(test)]
#[path = "kernel_reducer/command_apply_tests.rs"]
mod command_apply_tests;

#[cfg(test)]
#[path = "kernel_reducer/command_apply_publish_tests.rs"]
mod command_apply_publish_tests;
