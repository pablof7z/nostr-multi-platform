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
use crate::kernel::{Kernel, RelayFrame};
use crate::kernel_action::dispatch_kernel_action;
use crate::relay::{OutboundMessage, RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::substrate::SignedEvent;

/// Encapsulated kernel + public pure reducer.
///
/// Owns the [`Kernel`] privately so codegen-driven `FfiApp`s can reduce
/// [`KernelAction`] values to [`KernelUpdate`] values without depending on
/// crate-internal types.
pub struct KernelReducer {
    kernel: Kernel,
}

impl KernelReducer {
    /// Construct a fresh reducer with the default visible-limit. Equivalent
    /// to what the actor loop uses at startup.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kernel: Kernel::new(DEFAULT_VISIBLE_LIMIT),
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

    /// Pump the publish-engine retry queue. Mirrors the
    /// `tick_publish_engine_for_now` invocation the native actor performs on
    /// every inbound text frame (and on tick boundaries) — frames whose retry
    /// backoff has elapsed are returned for the caller to send.
    ///
    /// The wasm32 driver calls this from its `gloo-timers` periodic interval
    /// (1 Hz is sufficient; retry deadlines are seconds-scale) so transient
    /// publish failures recover without waiting for the next inbound frame
    /// from any relay.
    pub fn tick(&mut self) -> Vec<OutboundMessage> {
        let outbound = self.kernel.tick_publish_engine_for_now();
        self.kernel.partition_auth_paused(outbound)
    }

    /// V-01 Stage 3c — public publish-from-signed-event surface for non-actor
    /// consumers (today: the wasm32 `WasmRuntime` write path after the
    /// `Nip07Signer::sign()` Promise resolves; tomorrow: any in-process Rust
    /// caller that signs out-of-band and wants to feed the result through the
    /// kernel's publish engine).
    ///
    /// Internally delegates to `Kernel::publish_signed_with_correlation` —
    /// byte-for-byte the same entrypoint `actor::commands::publish::publish_note`
    /// reaches after `sign_active_nonblocking` resolves on the dispatched
    /// path. The returned `Vec<OutboundMessage>` is the engine's per-(outbox-
    /// relay) `EVENT` frame set, already AUTH-pause-partitioned through
    /// `partition_auth_paused` for symmetry with the `handle_relay_*` surface
    /// above.
    ///
    /// `p_tags` mirrors the legacy parameter on `Kernel::publish_signed` —
    /// callers that have no extra `#p` tags pass an empty slice. The engine
    /// recomputes `#p` tags from `signed.unsigned.tags` itself, so this slice
    /// is informational only (kept on the surface so a future caller that
    /// needs additional outbox routing tags has a place to inject them).
    ///
    /// `correlation_id` is the host-visible action id the publish should
    /// report in the `action_results` projection on terminal verdicts (per-
    /// relay OK / failed). Pass `Some(id)` when the publish is a dispatched
    /// action whose host caller is awaiting a terminal under `id` (the wasm
    /// runtime's `dispatch_app_action_async` Promise path); pass `None` for
    /// non-dispatch callers (the engine then reports the event id as the
    /// terminal key, matching every existing non-dispatched native publish).
    ///
    /// Without correlation threading the wasm host receives a publish-engine
    /// terminal keyed on an event id it never saw — defeating partial-success
    /// UX (e.g. "2/3 relays accepted"). Pinning the contract here keeps the
    /// wasm path byte-for-byte aligned with the native `publish_note` dispatch.
    ///
    /// Doctrine (D0/D6): the surface is substrate-typed (`SignedEvent`,
    /// `OutboundMessage`); failure is encoded as an empty outbound vec plus a
    /// kernel-side toast / `RecentFailure` row (no `Result` across this
    /// boundary, matching every other `KernelReducer` method).
    pub fn publish_signed_event(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
        correlation_id: Option<String>,
    ) -> Vec<OutboundMessage> {
        let outbound = self
            .kernel
            .publish_signed_with_correlation(signed, p_tags, correlation_id);
        self.kernel.partition_auth_paused(outbound)
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
}

impl Default for KernelReducer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::VIEW_PROFILE;
    use crate::nip19::encode_npub;

    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    #[test]
    fn reduce_open_uri_npub_routes_to_profile_view() {
        let mut r = KernelReducer::new();
        let npub = encode_npub(PK).unwrap();
        let update = r.reduce(KernelAction::OpenUri {
            uri: format!("nostr:{npub}"),
        });
        assert_eq!(
            update,
            KernelUpdate::ViewOpened {
                namespace: VIEW_PROFILE.into(),
                key: PK.into(),
            }
        );
    }

    #[test]
    fn reduce_start_echoes_started() {
        let mut r = KernelReducer::new();
        assert_eq!(
            r.reduce(KernelAction::Start),
            KernelUpdate::Started { rev: 0 }
        );
    }

    #[test]
    fn reduce_garbage_uri_is_rejected_not_a_panic() {
        let mut r = KernelReducer::new();
        let update = r.reduce(KernelAction::OpenUri {
            uri: "not-a-nostr-thing".into(),
        });
        assert!(matches!(
            update,
            KernelUpdate::UriRejected { reason, .. } if reason.contains("unparseable")
        ));
    }

    // ─── V-01 Stage 3 relay-lifecycle surface ────────────────────────────────
    //
    // These tests cover the contracts the wasm32 `BrowserRelayDriver` depends
    // on. They are intentionally narrow — the deep behaviour (replay
    // semantics, AUTH partition, wire-sub eviction) is already covered by the
    // kernel-side tests in `kernel/replay_tests.rs`, `kernel/auth_tests.rs`,
    // and `kernel/retention_tests.rs`. What we pin here is that
    // `KernelReducer` calls the right underlying methods in the right order
    // and never panics across the public surface.

    const RELAY: &str = "wss://relay.example";

    #[test]
    fn handle_relay_frame_text_does_not_panic_on_garbage() {
        // D6 invariant: a malformed NIP-01 frame must surface as a no-op
        // (the kernel silently drops unparseable text). The WASM driver
        // forwards every onmessage payload verbatim — we cannot assume
        // well-formedness.
        let mut r = KernelReducer::new();
        let out = r.handle_relay_frame(
            RelayRole::Content,
            RELAY,
            RelayFrame::Text("garbage that is not NIP-01".to_string()),
        );
        // No registered subs / publish engine state → empty outbound; the
        // important assertion is the absence of a panic.
        assert!(
            out.is_empty(),
            "garbage text must drop, not produce outbound"
        );
    }

    #[test]
    fn handle_relay_frame_close_does_not_panic() {
        let mut r = KernelReducer::new();
        let out = r.handle_relay_frame(
            RelayRole::Content,
            RELAY,
            RelayFrame::Close(Some("server going away".to_string())),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn handle_relay_frame_binary_and_ping_pong_are_counted_no_outbound() {
        let mut r = KernelReducer::new();
        for frame in [
            RelayFrame::Binary(b"opaque".to_vec()),
            RelayFrame::Ping,
            RelayFrame::Pong,
        ] {
            let out = r.handle_relay_frame(RelayRole::Indexer, RELAY, frame);
            assert!(out.is_empty(), "non-text frames must produce no outbound");
        }
    }

    #[test]
    fn handle_relay_connected_first_dial_emits_startup_or_empty() {
        // First-dial path (`is_reconnect = false`) on a fresh reducer with no
        // registered interests yields no startup REQs (`startup_requests`
        // returns empty until lifecycle.tick runs against a coverage plan).
        // The important contract: no panic and AUTH partition does not strip
        // legitimate frames.
        let mut r = KernelReducer::new();
        let _ = r.reduce(KernelAction::Start);
        let out = r.handle_relay_connected(RelayRole::Content, RELAY, false);
        // Empty is the correct answer for a kernel with no view-spec interests.
        assert!(out.is_empty(), "fresh kernel has no startup REQs");
    }

    #[test]
    fn handle_relay_connected_is_reconnect_does_not_panic() {
        let mut r = KernelReducer::new();
        let _ = r.reduce(KernelAction::Start);
        // First mark the relay closed so we have a valid "reconnect" state.
        r.handle_relay_closed(RelayRole::Content, RELAY);
        let _ = r.handle_relay_connected(RelayRole::Content, RELAY, true);
        // Pass: no panic.
    }

    #[test]
    fn handle_relay_failed_and_closed_are_total() {
        let mut r = KernelReducer::new();
        let _ = r.reduce(KernelAction::Start);
        r.handle_relay_failed(
            RelayRole::Content,
            RELAY,
            "connection reset by peer".to_string(),
        );
        r.handle_relay_closed(RelayRole::Content, RELAY);
        // Pass: no panic.
    }

    #[test]
    fn tick_on_fresh_reducer_is_empty() {
        // With no in-flight publishes, `tick_publish_engine_for_now` has
        // nothing to retry. AUTH partition over an empty vec is also empty.
        let mut r = KernelReducer::new();
        let _ = r.reduce(KernelAction::Start);
        assert!(r.tick().is_empty());
    }

    // ─── V-01 Stage 3c publish-from-signed-event surface ─────────────────────
    //
    // `publish_signed_event` is the new public seam the wasm runtime uses to
    // feed `Nip07Signer::sign()` results through the publish engine. The
    // tests here pin only the contract — total, no panic, returns an
    // outbound vec — and defer deep publish-engine behaviour to the
    // existing kernel-side tests in `publish/engine/tests.rs`.

    use crate::substrate::{SignedEvent, UnsignedEvent};

    fn synthetic_signed_note() -> SignedEvent {
        // Synthetic SignedEvent — the id and sig are placeholder hex strings
        // (the publish engine never re-verifies the signature; it just routes
        // the wire form). The kind:1 payload reaches the engine and goes
        // through NIP-65 outbox resolution, which on a fresh kernel with no
        // kind:10002 events in the store returns no targets and produces a
        // `NoTargets` `RecentFailure` row (empty outbound). That's exactly
        // the contract we want to assert: total, no panic, returns
        // `Vec::new()` rather than throwing.
        SignedEvent {
            id: "a".repeat(64),
            sig: "b".repeat(128),
            unsigned: UnsignedEvent {
                pubkey: PK.to_string(),
                kind: 1,
                tags: Vec::new(),
                content: "hello from wasm".to_string(),
                created_at: 1_700_000_000,
            },
        }
    }

    #[test]
    fn publish_signed_event_on_fresh_kernel_does_not_panic() {
        let mut r = KernelReducer::new();
        let _ = r.reduce(KernelAction::Start);
        let signed = synthetic_signed_note();
        // No kind:10002 known → engine records NoTargets → returns empty.
        // The important assertion is the absence of a panic; the empty-
        // outbound semantic is the documented D6 path.
        let out = r.publish_signed_event(&signed, &[], None);
        assert!(
            out.is_empty(),
            "fresh kernel has no NIP-65 outbox; publish must surface NoTargets, not outbound"
        );
    }

    #[test]
    fn publish_signed_event_accepts_empty_p_tags() {
        // The engine recomputes `#p` from `signed.unsigned.tags`; the slice
        // is informational. Pinning that empty is accepted is the smoke
        // test for the doc contract.
        let mut r = KernelReducer::new();
        let _ = r.reduce(KernelAction::Start);
        let signed = synthetic_signed_note();
        let _ = r.publish_signed_event(&signed, &[], None);
        // Pass: no panic.
    }

    #[test]
    fn publish_signed_event_threads_correlation_id_into_engine() {
        // The correlation_id parameter must reach the publish engine so
        // terminals land in `action_results` keyed on the dispatch id.
        // Without this, the wasm host receives terminals keyed on the
        // event id it never saw (partial-success UX would have no key to
        // correlate on). The contract is byte-identical with the native
        // `publish_note` dispatched path which uses
        // `Kernel::publish_signed_to_with_correlation`.
        //
        // We can't directly observe the engine's correlation_id table from
        // here (it's `pub(crate)`); the assertion below pins the surface
        // shape (no panic when correlation_id is `Some(_)`) — the deep
        // wire-up is exercised by the native `publish_note` tests in
        // `actor::commands::tests` and `publish::engine::tests`.
        let mut r = KernelReducer::new();
        let _ = r.reduce(KernelAction::Start);
        let signed = synthetic_signed_note();
        let _ = r.publish_signed_event(&signed, &[], Some("dispatch-1".to_string()));
        // Pass: no panic with Some correlation_id.
    }
}
