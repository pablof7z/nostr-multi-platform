//! Kernel ↔ `PublishEngine` wiring (T117).
//!
//! The publish engine (`crate::publish::PublishEngine`) is the per-(event,
//! relay) state machine that drives the publish retry FSM described in
//! `docs/research/relay-lifecycle-and-pools.md` §G5. Before T117 the engine
//! shipped but was dead code in production — `kernel::publish_cmd::publish_signed`
//! one-shotted a single `EVENT` frame and stamped `accepted_locally`. T117
//! routes every kernel publish through the engine instead.
//!
//! Doctrine map (canonical per `docs/product-spec/overview-and-dx.md` §1.5):
//! - **D3** (outbox automatic): the engine is built against
//!   `Nip65OutboxResolver`; every `Publish` uses `PublishTarget::Auto` so the
//!   resolver decides relays — no hardcoded URLs.
//! - **D4** (single writer per fact): only the kernel mutates engine state,
//!   only the engine mutates per-relay state. The actor holds the kernel
//!   one-thread, so the single-writer property is preserved.
//! - **D6** (no `Result` across FFI): every engine error is mapped into a
//!   `RecentFailure` snapshot row via `engine.record_engine_error` before the
//!   error propagates back across the kernel's plain-data return surface.
//! - **D7** (engine retries, native never decides): retry policy lives in
//!   the engine. The kernel only translates `OK` frames into `RelayAck`s and
//!   feeds them in via `on_ack`.
//! - **D8** (no per-event alloc on the resolve path): the `QueueDispatcher`
//!   appends to a single buffer; the kernel drains in bulk per publish call.

use std::sync::Arc;

use crate::publish::{
    Nip65OutboxResolver, NoopSigner, PublishAction, PublishEngine, PublishStore, PublishTarget,
    QueueDispatcher, RelayAck, RelayDispatcher, RetryPolicy,
};
use crate::relay::{OutboundMessage, RelayRole};
use crate::store::EventStore;
use crate::substrate::SignedEvent;

use super::publish_engine_wire::{describe_engine_error, now_epoch_ms, split_ok_message};
use super::Kernel;

/// Build the kernel's publish engine over a fresh `Nip65OutboxResolver` rooted
/// in the shared `EventStore`. The engine is mandatory on every Kernel
/// constructor.
pub(super) fn build_engine(
    event_store: Arc<dyn EventStore>,
    dispatcher: Arc<QueueDispatcher>,
    publish_store: Arc<dyn PublishStore>,
) -> PublishEngine {
    let resolver = Nip65OutboxResolver::with_default_fallback(event_store);
    PublishEngine::new(
        Arc::new(resolver),
        dispatcher as Arc<dyn RelayDispatcher>,
        publish_store,
        Arc::new(NoopSigner),
        RetryPolicy::default(),
    )
}

/// Coarse-grained `OK` payload extracted from a NIP-01 `["OK", id, ok, msg]`
/// frame. The kernel ingest pipeline only needs these three fields to map
/// into a publish-engine [`RelayAck`].
pub(crate) struct OkFramePayload<'a> {
    pub event_id: &'a str,
    pub ok: bool,
    pub message: &'a str,
}

impl Kernel {
    /// T117: drive a publish through the engine.
    ///
    /// One `PublishAction::Publish` → engine resolves NIP-65 → engine sends
    /// per-relay frames into the `QueueDispatcher` → kernel drains the buffer
    /// into `OutboundMessage`s (one per resolved relay). When the resolver
    /// returns no targets the engine produces a `RecentFailure` row and the
    /// kernel surfaces a `last_error_toast` (D6 — never an exception).
    ///
    /// Uses `event_id` as the publish handle: signers guarantee unique event
    /// ids per publish, so the handle <-> event_id collapse is sound and
    /// eliminates a reverse lookup map on the kernel side.
    pub(super) fn run_publish_engine(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine_at(signed, p_tags, now_epoch_ms())
    }

    /// Time-injected variant for deterministic tests. Production callers use
    /// `run_publish_engine` (which captures `SystemTime::now()`).
    pub(crate) fn run_publish_engine_at(
        &mut self,
        signed: &SignedEvent,
        _p_tags: &[String],
        now_ms: u64,
    ) -> Vec<OutboundMessage> {
        let handle = signed.id.clone();
        let action = PublishAction::Publish {
            handle: handle.clone(),
            event: signed.clone(),
            // D3: Auto target — the engine's `Nip65OutboxResolver` reads
            // kind:10002 from the shared event store. `_p_tags` is the
            // legacy parameter; the engine recomputes `#p` tags from
            // `event.unsigned.tags` itself, so we don't pass it through.
            target: PublishTarget::Auto,
        };
        let event_id = signed.id.clone();
        let kind = signed.unsigned.kind;
        match self.publish_engine.start_publish(action, now_ms) {
            Ok(()) => self.drain_publish_engine_frames(&event_id, kind),
            Err(err) => {
                // D6: map the engine error into a `RecentFailure` row on the
                // publish-status snapshot, set the kernel-level toast, and
                // record a queue entry so the projection reflects the failed
                // publish even when no frames went out.
                self.publish_engine
                    .record_engine_error(&err, &handle, &signed.id, now_ms);
                let (toast, status) = describe_engine_error(&err);
                self.set_last_error_toast(Some(toast));
                self.push_publish_entry(super::PublishQueueEntry {
                    event_id: signed.id.clone(),
                    kind: signed.unsigned.kind,
                    target_relays: 0,
                    status,
                });
                Vec::new()
            }
        }
    }

    /// Drain every frame the engine pushed into the queue dispatcher since the
    /// last drain, wrap each as a `Content`-lane outbound message, and update
    /// the per-publish queue projection.
    fn drain_publish_engine_frames(
        &mut self,
        event_id: &str,
        kind: u32,
    ) -> Vec<OutboundMessage> {
        let frames = self.publish_dispatcher.drain();
        let target_relays = frames.len();
        if frames.is_empty() {
            // Engine accepted the action but produced no synchronous frames
            // (every relay's `dispatch` returned empty acks under the
            // QueueDispatcher contract). This should not happen in practice
            // — `start_publish` always pushes through `dispatch_due`. Defensive
            // no-op for D6 (return cleanly, never assert).
            return Vec::new();
        }
        self.log(format!(
            "PUBLISH via engine kind:{} id={} → {} outbox relay(s)",
            kind,
            &event_id[..event_id.len().min(12)],
            target_relays
        ));
        // D5: the queue entry is the per-publish UI projection. Status
        // stays at `accepted_locally` (wire-shape preserved for iOS Pulse —
        // `ComposeView.swift` matches on this exact string). T117 refines
        // the *engine* truth (per-(event, relay) state survives ack); the
        // queue-entry status will get finer-grained terminal values
        // (`ok` / `failed`) in a follow-up that updates iOS in lockstep.
        self.push_publish_entry(super::PublishQueueEntry {
            event_id: event_id.to_string(),
            kind,
            target_relays,
            status: "accepted_locally".to_string(),
        });
        self.set_last_error_toast(None);
        self.changed_since_emit = true;
        frames
            .into_iter()
            .map(|(relay_url, text)| OutboundMessage {
                role: RelayRole::Content,
                relay_url,
                text,
            })
            .collect()
    }

    /// T117 ingest seam: parse a `["OK", id, ok, msg]` array off the wire,
    /// drop AUTH OKs (the AUTH driver consumed those upstream), and route
    /// publish OKs into the engine. Returns any retry frames the engine
    /// scheduled in response. `relay_url` is the resolved URL the OK
    /// arrived on — post-T105 the transport pool is URL-keyed, so this
    /// matches the URL the engine's `dispatch` produced.
    pub(crate) fn route_publish_ok(
        &mut self,
        relay_url: &str,
        array: &[serde_json::Value],
    ) -> Vec<OutboundMessage> {
        use nmp_nip42_types::parse_ok_frame;
        let Some(ok) = parse_ok_frame(array) else {
            return Vec::new();
        };
        // AUTH driver took the event_id-matching OK already; surviving OKs
        // are publishes. If the engine has no in-flight row for this event,
        // `on_ack` is a no-op (idempotent per D7).
        self.handle_publish_ok(
            relay_url,
            OkFramePayload {
                event_id: &ok.event_id,
                ok: ok.accepted,
                message: &ok.reason,
            },
        )
    }

    /// T117 ingest seam: fold a NIP-01 `OK` frame into the publish engine.
    ///
    /// Called from `route_publish_ok` (live wire path) and directly from
    /// integration tests that inject acks without going through the relay
    /// transport. `relay_url` is the resolved URL the ack arrived on — for
    /// the multi-URL-per-role future this comes from the inbound frame's
    /// connection identity, but today it's `role.url()`. The returned
    /// outbound is any retry the engine scheduled in response to a
    /// transient ack (drained from the queue dispatcher).
    pub(crate) fn handle_publish_ok(
        &mut self,
        relay_url: &str,
        payload: OkFramePayload<'_>,
    ) -> Vec<OutboundMessage> {
        self.handle_publish_ok_at(relay_url, payload, now_epoch_ms())
    }

    /// Time-injected variant for tests; production callers use the wall-clock
    /// `handle_publish_ok`.
    pub(crate) fn handle_publish_ok_at(
        &mut self,
        relay_url: &str,
        payload: OkFramePayload<'_>,
        now_ms: u64,
    ) -> Vec<OutboundMessage> {
        let ack = if payload.ok {
            RelayAck::ok(relay_url)
        } else {
            // NIP-20 OK-false: derive the engine `code` from the leading
            // colon-delimited prefix on the relay's message (e.g.
            // "blocked: spam" → `blocked`). Empty prefix → "error".
            let (code, message) = split_ok_message(payload.message);
            RelayAck::failed(relay_url, code, message)
        };
        // event_id == handle (per `run_publish_engine`).
        self.publish_engine
            .on_ack(&payload.event_id.to_string(), ack, now_ms);
        // Any retry the engine scheduled (after `Reauth` / transient backoff
        // that is already due) was pushed into the queue dispatcher; drain it.
        let drained = self.publish_dispatcher.drain();
        if !drained.is_empty() {
            self.changed_since_emit = true;
        }
        drained
            .into_iter()
            .map(|(url, text)| OutboundMessage {
                role: RelayRole::Content,
                relay_url: url,
                text,
            })
            .collect()
    }

    /// Wall-clock variant for the live ingest seam. Tests use the
    /// `tick_publish_engine(now_ms)` injection point directly.
    pub(crate) fn tick_publish_engine_for_now(&mut self) -> Vec<OutboundMessage> {
        self.tick_publish_engine(now_epoch_ms())
    }

    /// Drive the publish engine's wall-clock retries. Called from
    /// `kernel::ingest::handle_message` opportunistically (every inbound
    /// relay text frame ticks the engine, so the live path bounds retry latency
    /// by inbound traffic). Tests inject `now_ms` directly.
    pub(crate) fn tick_publish_engine(&mut self, now_ms: u64) -> Vec<OutboundMessage> {
        self.publish_engine.tick(now_ms);
        let drained = self.publish_dispatcher.drain();
        if !drained.is_empty() {
            self.changed_since_emit = true;
        }
        drained
            .into_iter()
            .map(|(url, text)| OutboundMessage {
                role: RelayRole::Content,
                relay_url: url,
                text,
            })
            .collect()
    }

    /// Resume any pending publishes that survived a kernel restart. Called by
    /// the actor (T127, `actor/dispatch.rs::Start`) once per `Start` command,
    /// and by integration tests directly. Returns any outbound frames the
    /// engine emitted as it brought live relays back into `InFlight` from a
    /// `Pending` / due-`RelayError` state.
    pub(crate) fn resume_publish_engine(&mut self) -> Vec<OutboundMessage> {
        let now_ms = now_epoch_ms();
        if let Err(err) = self.publish_engine.resume_from_store(now_ms) {
            // D6: durable-resume failure surfaces as a snapshot failure row
            // plus a toast; never a panic, never a `Result` across FFI.
            self.publish_engine
                .record_engine_error(&err, &String::new(), "", now_ms);
            let (toast, _) = describe_engine_error(&err);
            self.set_last_error_toast(Some(toast));
            return Vec::new();
        }
        let drained = self.publish_dispatcher.drain();
        drained
            .into_iter()
            .map(|(url, text)| OutboundMessage {
                role: RelayRole::Content,
                relay_url: url,
                text,
            })
            .collect()
    }

    /// Test/diagnostic accessor for the publish engine's snapshot. Exposed
    /// crate-private so integration tests can assert on `recent_ok` /
    /// `recent_errors` after driving the kernel through `publish_signed` +
    /// `handle_publish_ok`. The FFI-side projection bridge will read this
    /// through `make_update` in a follow-up wiring task.
    #[allow(dead_code)]
    pub(crate) fn publish_status_snapshot(
        &self,
    ) -> &crate::publish::PublishStatusSnapshot {
        self.publish_engine.snapshot()
    }
}
