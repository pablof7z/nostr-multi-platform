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
//! handles for each [`crate::relay_worker::RelayEvent`] variant. The wasm32
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
        self.kernel.relay_connected(role);
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

    /// Enqueue a pre-signed event through the publish engine. Returns the
    /// outbound frames the kernel wants sent immediately — one per resolved
    /// NIP-65 outbox relay (D3). Caller fans those out over its transport.
    ///
    /// V-01 Stage 3c — the wasm32 runtime calls this after
    /// `window.nostr.signEvent(...)` resolves: the signer hands back a
    /// `SignedEvent`, the runtime feeds it here, and the resulting
    /// `Vec<OutboundMessage>` fans out over the same `BrowserRelayDriver`
    /// pool the read path uses. Mirrors the native actor's
    /// `kernel.publish_signed(&signed, &[])` call (the actor takes the
    /// same dual-arity for replies — empty slice means "no extra `p` tags").
    /// Retry / ack / reauth lifecycle stays inside the engine; the caller
    /// only fans the immediate per-relay frames and lets later `OK`
    /// inbounds settle through `handle_relay_frame`.
    ///
    /// Ungated: mirrors `handle_relay_frame` / `handle_relay_connected` /
    /// `handle_relay_failed` / `handle_relay_closed` / `tick` — all
    /// unconditionally `pub` because `Kernel::publish_signed` itself is
    /// `pub(crate)` with no native gate (kernel/publish_cmd.rs:35), and
    /// the wasm32 build of nmp-core (`--no-default-features`) compiles
    /// every module the kernel touches.
    pub fn publish_signed_event(
        &mut self,
        signed: &crate::substrate::SignedEvent,
    ) -> Vec<OutboundMessage> {
        self.kernel.publish_signed(signed, &[])
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
        assert_eq!(r.reduce(KernelAction::Start), KernelUpdate::Started { rev: 0 });
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
        assert!(out.is_empty(), "garbage text must drop, not produce outbound");
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

    #[test]
    fn publish_signed_event_with_no_outbox_returns_empty_no_panic() {
        // V-01 Stage 3c contract: the wasm32 runtime hands a signed event in
        // (`SignedEvent` from `window.nostr.signEvent`) and expects a per-
        // relay outbound vec to fan out. With a fresh kernel and no NIP-65
        // outbox cached for the author, the publish engine resolves to zero
        // relays and the kernel records a `RecentFailure` (D6 toast surface).
        // The contract we pin here is that the call is total — no panic, the
        // returned vec is empty, and the reducer remains usable for further
        // calls.
        use crate::substrate::{SignedEvent, UnsignedEvent};

        let mut r = KernelReducer::new();
        let _ = r.reduce(KernelAction::Start);

        let signed = SignedEvent {
            id: "deadbeef".repeat(8),
            sig: "ab".repeat(32),
            unsigned: UnsignedEvent {
                pubkey: PK.to_string(),
                kind: 1,
                tags: Vec::new(),
                content: "hello, browser publish".to_string(),
                created_at: 1_700_000_000,
            },
        };

        let outbound = r.publish_signed_event(&signed);
        // No NIP-65 outbox is cached for this author on a fresh reducer; the
        // engine resolves to zero relays and the return is empty. The
        // important assertion is the absence of a panic.
        assert!(
            outbound.is_empty(),
            "fresh kernel with no NIP-65 outbox cache must return empty outbound"
        );

        // Reducer is still usable after the empty-outbox path.
        let _ = r.tick();
    }
}
