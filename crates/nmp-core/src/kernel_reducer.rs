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
//! to the same hand-written reducer the actor uses. Behaviour is byte-for-byte
//! identical with the actor path for every [`KernelAction`] variant,
//! including [`KernelAction::OpenUri`] (which registers a subscription
//! interest through the kernel's single-writer registry).
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
//!   Failures funnel into [`KernelUpdate::UriRejected`].
//! - **D8** — runs once per *action* / *frame*, not in a poll loop.
//!
//! This is the NMP-145 follow-up: T-NMP-145-FF.

use crate::app::{KernelAction, KernelUpdate};
use crate::kernel::{Kernel, RelayFrame, SnapshotProjectionSlot};
use crate::kernel_action::dispatch_kernel_action;
use crate::relay::{OutboundMessage, RelayRole, DEFAULT_VISIBLE_LIMIT};

/// Encapsulated kernel + public pure reducer.
///
/// Owns the [`Kernel`] privately so codegen-driven `FfiApp`s can reduce
/// [`KernelAction`] values to [`KernelUpdate`] values without depending on
/// crate-internal types. Two shared slots (`observer_slot`, `snapshot_slot`)
/// support the PR-4 wasm32 composition seams in `composition_seams.rs`.
pub struct KernelReducer {
    kernel: Kernel,
    /// Headless event-observer slot (no drain thread — wasm32 safe).
    observer_slot: crate::actor::KernelEventObserverSlot,
    /// Typed snapshot-projection slot.
    snapshot_slot: SnapshotProjectionSlot,
}

impl KernelReducer {
    /// Construct a fresh reducer with the default visible-limit. Equivalent
    /// to what the actor loop uses at startup.
    ///
    /// On all targets (including wasm32) this binds a headless
    /// [`KernelEventObserverSlot`] and a [`SnapshotProjectionSlot`] into the
    /// kernel so that composition roots can register event observers and typed
    /// projections without spawning background threads.
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
        }
    }

    /// Reduce one [`KernelAction`] against the encapsulated kernel, returning
    /// the [`KernelUpdate`] the host app should observe.
    ///
    /// Total and panic-free (D6): the only fallible action (`OpenUri`)
    /// funnels its typed error into [`KernelUpdate::UriRejected`].
    pub fn reduce(&mut self, action: KernelAction) -> KernelUpdate {
        dispatch_kernel_action(&mut self.kernel, action)
    }

    // ─── V-01 Stage 3 relay-lifecycle surface ────────────────────────────────
    //
    // These methods mirror the per-event arms of
    // `actor::dispatch::handle_relay_event` so a non-actor consumer (the
    // wasm32 `BrowserRelayDriver`) can drive the same kernel state machine.
    // Each method returns the outbound the kernel wants sent immediately — the
    // caller fans those out over its transport. There is no central outbound
    // queue inside the kernel; producers return frames directly. The actor
    // captures these per-call, and so must the WASM driver.
    //
    // AUTH-pause partitioning is applied before returning so a frame addressed
    // to a relay currently mid-NIP-42-handshake is buffered inside the kernel
    // and replayed on the next tick after `Authenticated` — matching the
    // native `send_all_outbound` invariant. The caller does not need to know
    // the AUTH state machine exists.

    /// One inbound relay frame on `(role, relay_url)`. Mirrors the
    /// `RelayEvent::Message` arm of the native actor: routes through
    /// [`Kernel::handle_message`], appends [`Kernel::pending_view_requests`]
    /// (newly-registered subs that need a wire REQ now that we have a socket
    /// to leave on), and partitions the result through the NIP-42 AUTH-pause
    /// gate before returning.
    ///
    /// V-01 Stage 3 — the wasm32 `BrowserRelayDriver` calls this from its
    /// `WebSocket::onmessage` closure for every text/binary frame and from
    /// its `oncloseevent` closure for [`RelayFrame::Close`]. `RelayFrame::Ping`
    /// and `RelayFrame::Pong` are accepted and bump the keepalive frame
    /// counter; the driver still maintains its own client-side ping cadence
    /// on a `gloo-timers` interval (the kernel never produces outbound pings).
    pub fn handle_relay_frame(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        frame: RelayFrame,
    ) -> Vec<OutboundMessage> {
        let mut outbound = self.kernel.handle_message(role, relay_url, frame);
        outbound.extend(self.kernel.pending_view_requests());
        self.kernel.partition_auth_paused(outbound)
    }

    /// A relay socket entered the `connected` state. Mirrors the
    /// `RelayEvent::Connected` arm: flips the per-lane `RelayStatus`
    /// connection field, emits any startup REQs that were waiting on a socket,
    /// and replays publish-engine frames whose target relay just became
    /// available.
    ///
    /// `is_reconnect == true` triggers the same re-emission of active
    /// subscription shapes the native `replay_on_reconnect` path applies
    /// (T116/G1) — the wire-subs map for this URL was evicted by the prior
    /// `Closed` and the relay's per-connection sub-id table is fresh, so
    /// every active shape must be re-REQed with its T129 watermark.
    ///
    /// The returned `Vec<OutboundMessage>` is already AUTH-pause-partitioned.
    pub fn handle_relay_connected(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        is_reconnect: bool,
    ) -> Vec<OutboundMessage> {
        self.kernel.relay_connected_url(role, relay_url);
        let mut outbound = Vec::new();
        if is_reconnect {
            // Same call shape the native actor uses; `replay_on_reconnect`
            // is a pure read of `SubscriptionLifecycle::handle_reconnect` and
            // never panics.
            outbound.extend(self.kernel.replay_on_reconnect(role, relay_url));
        }
        outbound.extend(self.kernel.mark_publish_relay_available(relay_url));
        outbound.extend(self.kernel.startup_requests());
        outbound.extend(self.kernel.pending_view_requests());
        // V-04 Stage 2: `startup_requests` no longer emits M1 `OutboundMessage`
        // frames for the four bootstrap interests (self profile / NIP-65 /
        // NIP-17 DM relays / contacts) — it now registers them through
        // `InterestRegistry::ensure_sub` and enqueues a
        // `CompileTrigger::ViewOpened`. The native actor drains the lifecycle
        // on its idle loop; the wasm `KernelReducer` has no such loop, so we
        // drain inline here. Empty diff is a zero-cost no-op (D8).
        outbound.extend(self.kernel.drain_lifecycle_outbound());
        self.kernel.partition_auth_paused(outbound)
    }

    /// A relay socket failed transiently (the transport will retry). Mirrors
    /// the `RelayEvent::Failed` arm: marks the per-URL wire-subs as
    /// `retrying` and surfaces the error on the next snapshot. Returns no
    /// outbound (the kernel never emits replies to a failed connection;
    /// queued frames are deferred until the next `Connected`).
    pub fn handle_relay_failed(&mut self, role: RelayRole, relay_url: &str, error: String) {
        self.kernel.relay_failed(role, relay_url, error);
        self.kernel.mark_publish_relay_unavailable(relay_url);
    }

    /// A relay socket was torn down (no retry). Mirrors the `RelayEvent::Closed`
    /// arm: evicts every wire-sub keyed on this URL (T133) and resets the NIP-42
    /// driver for the role lane. Returns no outbound.
    pub fn handle_relay_closed(&mut self, role: RelayRole, relay_url: &str) {
        self.kernel.relay_closed(role, relay_url);
        self.kernel.mark_publish_relay_unavailable(relay_url);
    }

    /// Pump all four idle drains in native parity order: pending view
    /// requests → lifecycle drain → claim-expansion tick → publish pump.
    /// Mirrors the native actor's idle-tick sequence at
    /// `actor/mod.rs:2086–2098` (pending_view_requests), `2107–2120`
    /// (lifecycle drain), `2164–2188` (claim-expansion W6), and `2161–2173`
    /// (publish pump).
    ///
    /// The wasm32 driver calls this from its `gloo-timers` periodic interval
    /// (1 Hz is sufficient; retry deadlines are seconds-scale) so transient
    /// publish failures recover without waiting for the next inbound frame
    /// from any relay, `CompileTrigger::ViewOpened` events enqueued by
    /// `claim_event` / `claim_profile` compile into REQ frames without
    /// waiting for the next relay event, and Phase-1 claims advance to
    /// Phase 2 on every tick rather than stalling permanently on quiet
    /// sockets (closes the W6 gap tracked in issue #1143).
    ///
    /// `pending_view_requests()` is placed first (before the lifecycle drain)
    /// to preserve the native M1-CLOSE-before-M2-REQ ordering: any pending
    /// M1 CLOSE frames must be enqueued before the M2 planner opens new subs
    /// (spec §3.1 placement rationale, also documented at the lifecycle-drain
    /// site in `actor/mod.rs:2099–2106`). It also ensures time-gated work
    /// (`contacts_deadline`, F-TTL `drain_pending_reverify`, deferred AUTH-gate
    /// REQs) fires on every idle tick rather than only when inbound traffic
    /// arrives. Note: `pending_view_requests()` internally calls
    /// `maybe_open_timeline()` which uses `Instant::now()`, but that path
    /// already runs on every inbound frame via `handle_relay_frame` (line 114),
    /// so this call adds no new wasm32 `Instant` exposure.
    ///
    pub fn tick(&mut self) -> Vec<OutboundMessage> {
        // 1. Pending view requests: mirrors actor/mod.rs:2086-2098.
        //    Drains time-gated work (contacts_deadline, F-TTL reverify, deferred
        //    AUTH-gate REQs from deferred_outbound) that would otherwise only
        //    fire when inbound traffic arrives on a quiet socket.
        let mut outbound = self.kernel.pending_view_requests();
        // 2. Lifecycle drain: mirrors actor/mod.rs:2107-2120.
        //    Compiles queued CompileTriggers into REQ/CLOSE WireFrames.
        //    Placed after pending_view_requests to ensure M1 CLOSE frames are
        //    enqueued before M2 opens new subs (spec §3.1).
        outbound.extend(self.kernel.drain_lifecycle_outbound());
        // 3. Claim-expansion idle tick: mirrors actor/mod.rs:2164-2188 (W6).
        //    Advances the per-claim Phase-1/2 state machine once per tick.
        //    `crate::time::Instant` resolves to `web_time::Instant` on
        //    wasm32-unknown-unknown (performance.now() backed) and to
        //    `std::time::Instant` on native — both are panic-free on their
        //    respective targets (closes the #1143 / #1009 blocker).
        //    D8: with no pending claims this is a single `is_empty()` check;
        //    no allocation, no iteration.
        outbound.extend(self.kernel.poll_claim_expansion(crate::time::Instant::now()));
        // 4. Publish pump: mirrors actor/mod.rs:2161-2173.
        //    Retries in-flight publish frames whose retry deadline has elapsed.
        outbound.extend(self.kernel.tick_publish_engine_for_now());
        self.kernel.partition_auth_paused(outbound)
    }

    /// Returns `true` when the kernel state has changed since the last
    /// `make_update_frame` call. The wasm32 periodic timer checks this before
    /// pushing a snapshot so that idle ticks do not produce spurious frames
    /// (dirty-flag coalescing, PR-2 rider).
    pub fn changed_since_emit(&self) -> bool {
        self.kernel.changed_since_emit()
    }

    // ─── F-CR-00 component-owned claim seam ─────────────────────────────────
    //
    // Wasm consumers (chirp-web components) have no ActorCommand channel —
    // they drive the kernel through `KernelReducer` directly. These four
    // methods expose the same `Kernel::claim_profile` / `release_profile` /
    // `claim_event` / `release_event` surface the actor uses on native, so
    // web components can self-claim profiles and events on mount/unmount the
    // same way iOS (`chirp-avatar.<uuid>`) and Android (`note-author-<eventId>`)
    // do.
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

    /// Refcount a consumer's interest in `pubkey`'s kind:0 profile. On the
    /// cold-claim transition emits the batched-REQ `OutboundMessage`(s) the
    /// caller should fan to connected relays.
    ///
    /// `can_send` should be `self.kernel.any_relay_connected()` at the call
    /// site (`KernelReducer::any_relay_connected` exposes this). When
    /// `can_send = false` the claim parks in `profile_requests.pending` and
    /// is drained by `handle_relay_connected` → `pending_view_requests` on
    /// the next relay connect event, or by the periodic `tick()` if the relay
    /// is already connected when the claim arrives.
    pub fn claim_profile(
        &mut self,
        pubkey: String,
        consumer_id: String,
        can_send: bool,
        force: bool,
    ) -> Vec<OutboundMessage> {
        let outbound = self
            .kernel
            .claim_profile(pubkey, consumer_id, can_send, force);
        self.kernel.partition_auth_paused(outbound)
    }

    /// Drop a consumer's refcounted interest in `pubkey`'s kind:0 profile.
    /// When the last consumer releases, the pending-request entry is removed.
    /// Returns an empty vec (release never emits wire frames).
    pub fn release_profile(&mut self, pubkey: &str, consumer_id: &str) -> Vec<OutboundMessage> {
        let outbound = self.kernel.release_profile(pubkey, consumer_id);
        self.kernel.partition_auth_paused(outbound)
    }

    /// Refcount a consumer's interest in the event identified by `uri`
    /// (a `nostr:nevent1…` / `nostr:note1…` / `nostr:naddr1…` URI). On the
    /// cold-claim transition registers a `OneShot + Global` lifecycle interest
    /// and enqueues a `CompileTrigger::ViewOpened` so the planner compiles a
    /// REQ. Returns any immediately-sendable `OutboundMessage`(s).
    ///
    /// Malformed URIs are silently dropped (D6: no panic, no `Result`).
    pub fn claim_event(
        &mut self,
        uri: String,
        consumer_id: String,
        can_send: bool,
        force: bool,
    ) -> Vec<OutboundMessage> {
        let outbound = self.kernel.claim_event(uri, consumer_id, can_send, force);
        self.kernel.partition_auth_paused(outbound)
    }

    /// Drop a consumer's refcounted interest in the event identified by `uri`.
    /// Returns an empty vec (release never emits wire frames).
    ///
    /// Malformed URIs are silently dropped (D6).
    pub fn release_event(&mut self, uri: &str, consumer_id: &str) -> Vec<OutboundMessage> {
        let outbound = self.kernel.release_event(uri, consumer_id);
        self.kernel.partition_auth_paused(outbound)
    }

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
    /// Wasm-side accessors (e.g. `nmp-wasm`'s test helpers) use this to
    /// verify that `set_active_account` stored the canonicalised form.
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
    /// Wasm-friendly read seam — the `nmp-wasm` runtime exposes this to JS
    /// hosts (`NmpWasmRuntime::recent_routing_decisions`) so the web Chirp
    /// shell can render the same routing inspector iOS gets via the
    /// `nmp_app_recent_routing_decisions` FFI symbol. Native callers reach
    /// the projection directly through [`crate::Kernel::routing_trace`].
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

    /// Populate the kernel's configured-relay lanes from a caller-supplied
    /// list of `(url, role)` pairs.
    ///
    /// Each `role` string is canonicalised via the kernel's own
    /// `canonical_relay_role` pass (same normalisation the native actor
    /// applies on every relay-edit write). [`crate::kernel::AppRelay`] is
    /// `pub(crate)`; external crates (e.g. `nmp-wasm`) pass raw string
    /// pairs and let this method build the typed rows internally.
    ///
    /// Calling this before the first [`make_update_frame`] ensures the
    /// `relay_statuses` Tier-3 rows and the `configured_relays` typed
    /// projection both carry real URLs rather than empty defaults.
    ///
    /// [`make_update_frame`]: Self::make_update_frame
    pub fn set_configured_relays(&mut self, rows: Vec<(String, String)>) {
        use crate::kernel::AppRelay;
        let relay_rows: Vec<AppRelay> = rows
            .into_iter()
            .map(|(url, role)| AppRelay::new(url, role))
            .collect();
        self.kernel.set_configured_relays(relay_rows);
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

mod composition_seams;
mod feed_verbs;
mod follow;
mod react;
mod reply;

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
