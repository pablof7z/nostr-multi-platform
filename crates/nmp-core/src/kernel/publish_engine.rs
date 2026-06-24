//! Kernel ↔ `PublishEngine` wiring (T117).
//!
//! The publish engine (`crate::publish::PublishEngine`) is the per-(event,
//! relay) state machine that drives the publish retry FSM described in
//! `docs/research/relay-lifecycle-and-pools.md` §G5. Before T117 the engine
//! shipped but was dead code in production — `kernel::publish_cmd::publish_signed`
//! one-shotted a single `EVENT` frame and stamped `accepted_locally`. T117
//! routes every kernel publish through the engine instead.
//!
//! Doctrine map (canonical per `docs/product-spec/doctrine.md`):
//! - **D3** (outbox automatic): the engine is built against an
//!   `Arc<dyn OutboxResolver>` slot (default: `NoopOutboxResolver`);
//!   production composition (`nmp-defaults::register_defaults`) installs
//!   the router-side `nmp_router::Nip65OutboxResolver` via
//!   [`Kernel::set_publish_resolver`]. Every `Publish` uses
//!   `PublishTarget::Auto` so the installed resolver decides relays — no
//!   hardcoded URLs. With the default `NoopOutboxResolver` the engine
//!   surfaces `NoTargets` (fail-closed), exactly the same as the
//!   `Nip65OutboxResolver` does for an author with no kind:10002.
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

// `publish_engine_terminals` declared here (rather than in `kernel/mod.rs`) to
// keep the parent module file untouched — V-12 hand-authored ceiling. The
// child's `impl Kernel` block reaches the kernel via `super::Kernel`.
#[cfg(test)]
#[path = "publish_engine_local_fallback_tests.rs"]
mod local_fallback_tests;
#[path = "publish_engine_runtime.rs"]
mod runtime;
#[path = "publish_engine_terminals.rs"]
mod terminals;

use crate::publish::{
    NoopOutboxResolver, NoopSigner, OutboxResolver, PublishAction, PublishEngine, PublishStore,
    PublishTarget, QueueDispatcher, RelayAck, RelayDispatcher, RetryPolicy,
};
use crate::relay::{OutboundMessage, RelayRole};
use crate::substrate::SignedEvent;

use super::publish_engine_wire::{describe_engine_error, split_ok_message};
use super::Kernel;

/// Build the kernel's publish engine with the in-crate `NoopOutboxResolver`
/// default. Production composition (`nmp-defaults::register_defaults`)
/// swaps in the router-side `nmp_router::Nip65OutboxResolver` via
/// [`Kernel::set_publish_resolver`] before any publish lands — until then
/// every `PublishTarget::Auto` resolves to an empty set and the engine emits
/// `NoTargets` (fail-closed by default, exactly as the production
/// `Nip65OutboxResolver` does for an uncached author).
///
/// Spec §271 (2026-05-25): `Nip65OutboxResolver` was moved out of
/// `nmp_core::publish::nip65` into `nmp_router` so the substrate stays
/// NIP-neutral (D0). The kernel cannot name the router-side type (Layer 3
/// → Layer 2 inverts the dependency arrow), so the injection flows through
/// the `NmpApp::set_publish_resolver_factory` slot the actor reads at
/// kernel construction time.
pub(super) fn build_engine(
    dispatcher: Arc<QueueDispatcher>,
    publish_store: Arc<dyn PublishStore>,
) -> PublishEngine {
    let resolver: Arc<dyn OutboxResolver> = Arc::new(NoopOutboxResolver);
    PublishEngine::new(
        resolver,
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
    /// ids per publish, so the handle <-> `event_id` collapse is sound and
    /// eliminates a reverse lookup map on the kernel side.
    pub(super) fn run_publish_engine(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
        target: PublishTarget,
        correlation_id_override: Option<String>,
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine_at(
            signed,
            p_tags,
            target,
            correlation_id_override,
            self.now_ms(),
        )
    }

    /// Time-injected variant for deterministic tests. Production callers use the
    /// kernel-owned clock through `run_publish_engine`.
    ///
    /// `target` selects the relay-resolution mode (D3): `Auto` defers to the
    /// `Nip65OutboxResolver` (kind:10002 outbox); `Explicit { relays }` is the
    /// named opt-out and routes the verbatim event to exactly those relays.
    ///
    /// `correlation_id_override` is the action `correlation_id` to report in
    /// `action_results` instead of the publish handle (== event id). It is
    /// `Some` only on the `PublishRaw` dispatch path — the host received a
    /// registry-minted id before the actor signed the event, so the engine
    /// must report that id, not the event's. Every other caller passes `None`.
    pub(crate) fn run_publish_engine_at(
        &mut self,
        signed: &SignedEvent,
        _p_tags: &[String],
        target: PublishTarget,
        correlation_id_override: Option<String>,
        now_ms: u64,
    ) -> Vec<OutboundMessage> {
        // Workstream C publish-policy one-door (D10 structural gate). EVERY
        // signed publish — dispatched or internal, signed-event or
        // unsigned-then-signed — funnels through this engine entry, so it is
        // the single structural chokepoint where the private-envelope
        // fail-closed invariant is enforced. A private/encrypted kind
        // (gift-wrap kind:1059, sealed kind:14) with `Auto` or an empty
        // `Explicit` target is REFUSED here before any outbound frame or queue
        // entry is produced, so a private event can never leak to the author's
        // public relays — regardless of which path reached the engine. The
        // (kind, target) → allow/reject decision is the publish-policy table's;
        // this site only consults it (no raw kind literal here).
        if let Err(reason) = crate::publish::validate_publish_routing(
            signed.unsigned.kind,
            crate::publish::target_is_explicit_nonempty(&target),
        ) {
            tracing::warn!(
                kind = signed.unsigned.kind,
                "run_publish_engine refused: private/encrypted envelope without an \
                 explicit relay pin would route through the author's public-relay \
                 outbox (D10 violation). Caller must supply PublishTarget::Explicit \
                 with a non-empty recipient-inbox relay set."
            );
            self.set_last_error_toast(Some(reason.clone()));
            // Broken-promise fix: a dispatched action carries a correlation_id
            // and the host is waiting on `action_results`; record the terminal
            // failure so its spinner clears. No-op for `None` (internal /
            // conformance callers have nothing waiting on an id).
            if let Some(id) = correlation_id_override {
                // Curated kernel policy copy (the D10 routing-leak refusal) — the
                // host localizes it (#1735). Mirrors the action-layer chokepoint
                // in `actor/commands/publish.rs` which already codes this path.
                let code = crate::ui_token::codes::LIFECYCLE_PUBLISH_NO_EXPLICIT_TARGET;
                self.record_action_failure_coded(id, reason, Some(code), None);
            }
            return Vec::new();
        }
        let handle = signed.id.clone();
        let action = PublishAction::Publish {
            handle: handle.clone(),
            event: signed.clone(),
            // D3: `target` is `Auto` for every existing caller (the engine's
            // `Nip65OutboxResolver` reads kind:10002 from the shared event
            // store) or the `Explicit` opt-out for gift-wrap and similar
            // routing. `_p_tags` is the legacy parameter; the engine
            // recomputes `#p` tags from `event.unsigned.tags` itself, so we
            // don't pass it through.
            target: target.clone(),
        };
        let event_id = signed.id.clone();
        // Cloned before the move into `start_publish` so the `Err` arm can
        // still honour the dispatch correlation_id (broken-promise fix).
        let correlation_id_for_failure = correlation_id_override.clone();
        let engine_rev_before = self.publish_engine.snapshot().rev;
        match self
            .publish_engine
            .start_publish(action, now_ms, correlation_id_override.clone())
        {
            Ok(()) => {
                // S7 (#1754): this is the single engine-entry site that knows
                // BOTH the publish handle (== event id) and the original
                // dispatch correlation_id. Record the durable handle↔correlation
                // index so a later cancel-by-correlation-id can reverse-resolve
                // the handle AND land the `Cancelled` terminal under the
                // ORIGINAL correlation_id (PD-036). `None` maps the handle to
                // itself, preserving cancel-by-handle for internal publishes.
                self.publish_handle_correlation
                    .record(&event_id, correlation_id_override.as_deref());
                // A `correlation_id`-bearing publish reached the engine's
                // accept path — record `Publishing` so the host's stage
                // mirror reflects the lifecycle transition. The detail
                // payload carries the event id for per-publish progress UI.
                // Non-dispatch publishes (the `None` branch) skip this:
                // there is no host spinner to inform.
                if let Some(cid) = correlation_id_override.as_ref() {
                    self.record_action_stage(
                        cid,
                        super::action_stages::ActionStage::Publishing,
                        Some(serde_json::json!({ "event_id": event_id })),
                    );
                }
                // ADR-0057 — route the locally-published event through the
                // single accepted-event chokepoint with `local://publish`
                // provenance, exactly as a relay-delivered event flows through
                // it. This gives read-your-writes for ALL kinds (incl. kind:1
                // notes / kind:6 reposts / kind:7 reactions — #1440), not just
                // the replaceables the deleted `record_local_publish_intent`
                // mirror ladder covered: the chokepoint persists the event
                // (valid-sig admission) and fires the app-observer + NIP-parser
                // delivery + the timeline projection immediately. The relay echo
                // later dedups to `Duplicate` and is projection-silent (D4), so
                // observers fire exactly once.
                let local_event = super::nostr::signed_event_to_nostr(signed);
                let _ = self
                    .ingest_accepted_event(super::ingest::IngestSource::LocalPublish, local_event);
                let frames = self.drain_publish_engine_frames(signed, target);
                // Synchronous dispatchers (e.g. some test fixtures) can settle
                // a publish inside `start_publish` itself by returning OK acks
                // from `dispatch_due`. Drain any terminal verdicts that
                // produced so the queue entry never lingers at
                // `accepted_locally` past the engine's view.
                self.drain_engine_terminals_into_ledger();
                self.bump_publish_if_engine_view_changed(engine_rev_before);
                frames
            }
            Err(err) => {
                // D6: map the engine error into a `RecentFailure` row on the
                // publish-status snapshot, set the kernel-level toast, and
                // record a queue entry so the projection reflects the failed
                // publish even when no frames went out.
                self.publish_engine
                    .record_engine_error(&err, &handle, &signed.id, now_ms);
                let (toast, status, category) = describe_engine_error(&err);
                // S11 slice 2 (#1758): fold any engine-origin terminal the failed
                // `start_publish` already pushed (the `NoTargets` path's
                // `emit_no_targets` records a `"failed"` verdict on
                // `pending_terminals`) into the ledger NOW, BEFORE the off-band
                // `record_action_failure` below. This preserves the prior
                // producer order — the engine `emit_no_targets` row precedes the
                // engine-error broken-promise row — now that the off-band path
                // records straight into the ledger instead of onto the same
                // engine `Vec`. A no-op for errors that pushed no pending
                // terminal (`DuplicateHandle`, `Store`, `UnsupportedAction`).
                self.drain_engine_terminals_into_ledger();
                // Broken-promise fix: an engine-level error (`DuplicateHandle`,
                // `Store`, `UnsupportedAction`) for a dispatched action — one
                // that carries a `correlation_id_override` — must also reach
                // `action_results` so the host spinner clears. `record_engine_error`
                // above writes only a `RecentFailure` row, not a terminal
                // action verdict. `None` (a non-dispatch publish) is a no-op.
                if let Some(id) = correlation_id_for_failure {
                    self.record_action_failure(id, toast.clone());
                }
                self.set_error_toast_with_category(toast, category);
                self.push_publish_entry(super::PublishQueueEntry {
                    event_id: signed.id.clone(),
                    kind: signed.unsigned.kind,
                    target_relays: 0,
                    can_retry: status == "pending_relays_unknown",
                    status,
                    relay_outcomes: Vec::new(),
                    signed_event: Some(signed.clone()),
                    target: Some(target),
                });
                self.bump_publish_if_engine_view_changed(engine_rev_before);
                Vec::new()
            }
        }
    }

    /// Drain every frame the engine pushed into the queue dispatcher since the
    /// last drain, wrap each as a `Content`-lane outbound message, and update
    /// the per-publish queue projection.
    fn drain_publish_engine_frames(
        &mut self,
        signed: &SignedEvent,
        target: PublishTarget,
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
        let event_id = signed.id.as_str();
        let kind = signed.unsigned.kind;
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
            can_retry: false,
            // Empty until the engine settles — T128 fills this via the engine
            // terminal fold (`drain_engine_terminals_into_ledger`) once the
            // per-relay state machine reaches a terminal verdict (S11 slice 4).
            relay_outcomes: Vec::new(),
            signed_event: Some(signed.clone()),
            target: Some(target),
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
        self.handle_publish_ok_at(relay_url, payload, self.now_ms())
    }

    /// Time-injected variant for tests; production callers use the kernel-owned
    /// clock through `handle_publish_ok`.
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
        let engine_rev_before = self.publish_engine.snapshot().rev;
        self.publish_engine
            .on_ack(&payload.event_id.to_string(), ack, now_ms);
        // T128: a terminal ack (Ok or final give-up) may have just settled
        // the publish — apply the terminal verdict to the queue entry before
        // any retry frame drain so the iOS snapshot reflects the new status.
        self.drain_engine_terminals_into_ledger();
        // Any retry the engine scheduled (transient backoff that is already
        // due) was pushed into the queue dispatcher; drain it. An auth-required
        // ack parks the relay instead (no synchronous frame here — the
        // re-dispatch fires later off the `Authenticated` availability gate).
        let drained = self.publish_dispatcher.drain();
        if !drained.is_empty() {
            self.changed_since_emit = true;
        }
        self.bump_publish_if_engine_view_changed(engine_rev_before);
        drained
            .into_iter()
            .map(|(url, text)| OutboundMessage {
                role: RelayRole::Content,
                relay_url: url,
                text,
            })
            .collect()
    }
}
