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

use std::sync::{Arc, Mutex};

use crate::publish::{
    Nip65OutboxResolver, NoopSigner, PublishAction, PublishEngine, PublishStore, PublishTarget,
    QueueDispatcher, RelayAck, RelayDispatcher, RetryPolicy, TerminalOutcome,
};
use crate::relay::{OutboundMessage, RelayRole};
use crate::store::EventStore;
use crate::substrate::SignedEvent;

use super::publish_engine_wire::{describe_engine_error, now_epoch_ms, split_ok_message};
use super::{IndexerRelaysSlot, Kernel, LocalWriteRelaysSlot};

/// Build the kernel's publish engine over a fresh `Nip65OutboxResolver` rooted
/// in the shared `EventStore`. `indexer_relays` is a shared handle the kernel
/// keeps in sync with its relay config; the resolver reads it on every publish
/// so discovery-kind fan-out always uses current URLs.
///
/// PR-I: the `indexer_relays` / `local_write_relays` handles are now typed
/// slots ([`IndexerRelaysSlot`] / [`LocalWriteRelaysSlot`]). The previous
/// bare `Arc<Mutex<Vec<String>>>` shape would trip the new D14 lint on any
/// future field declaration; threading the typed alias through the
/// constructor keeps the call-site shape uniform with the kernel field.
pub(super) fn build_engine(
    event_store: Arc<dyn EventStore>,
    dispatcher: Arc<QueueDispatcher>,
    publish_store: Arc<dyn PublishStore>,
    indexer_relays: IndexerRelaysSlot,
    local_write_relays: LocalWriteRelaysSlot,
    active_account: Arc<Mutex<Option<String>>>,
) -> PublishEngine {
    let resolver = Nip65OutboxResolver::with_local_relays(
        event_store,
        indexer_relays,
        local_write_relays,
        active_account,
    );
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
        target: PublishTarget,
        correlation_id_override: Option<String>,
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine_at(
            signed,
            p_tags,
            target,
            correlation_id_override,
            now_epoch_ms(),
        )
    }

    /// Time-injected variant for deterministic tests. Production callers use
    /// `run_publish_engine` (which captures `SystemTime::now()`).
    ///
    /// `target` selects the relay-resolution mode (D3): `Auto` defers to the
    /// `Nip65OutboxResolver` (kind:10002 outbox); `Explicit { relays }` is the
    /// named opt-out and routes the verbatim event to exactly those relays.
    ///
    /// `correlation_id_override` is the action correlation_id to report in
    /// `action_results` instead of the publish handle (== event id). It is
    /// `Some` only on the `PublishNote` dispatch path — the host received a
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
            target,
        };
        let event_id = signed.id.clone();
        let kind = signed.unsigned.kind;
        // Cloned before the move into `start_publish` so the `Err` arm can
        // still honour the dispatch correlation_id (broken-promise fix).
        let correlation_id_for_failure = correlation_id_override.clone();
        match self
            .publish_engine
            .start_publish(action, now_ms, correlation_id_override.clone())
        {
            Ok(()) => {
                // PR-G: a `correlation_id`-bearing publish reached the
                // engine's accept path — record `Publishing` so the host's
                // stage mirror reflects the lifecycle transition. The
                // detail payload carries the event id so a host that
                // displays a per-publish progress label has the data.
                // Non-dispatch publishes (the `None` branch) skip this:
                // there is no host spinner to inform.
                if let Some(cid) = correlation_id_override.as_ref() {
                    self.record_action_stage(
                        cid,
                        super::action_stages::ActionStage::Publishing,
                        Some(serde_json::json!({ "event_id": event_id })),
                    );
                }
                self.record_local_publish_intent(signed);
                let frames = self.drain_publish_engine_frames(&event_id, kind);
                // Synchronous dispatchers (e.g. some test fixtures) can settle
                // a publish inside `start_publish` itself by returning OK acks
                // from `dispatch_due`. Drain any terminal verdicts that
                // produced so the queue entry never lingers at
                // `accepted_locally` past the engine's view.
                self.apply_engine_completions();
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
                // Broken-promise fix: an engine-level error (`DuplicateHandle`,
                // `Store`, `UnsupportedAction`) for a dispatched action — one
                // that carries a `correlation_id_override` — must also reach
                // `action_results` so the host spinner clears. `record_engine_error`
                // above writes only a `RecentFailure` row, not a terminal
                // action verdict. (`NoTargets` does not reach here — it is a
                // terminal handled by `emit_no_targets`, which records its own
                // verdict.) `None` (a non-dispatch publish) is a no-op.
                if let Some(id) = correlation_id_for_failure {
                    self.record_action_failure(id, toast.clone());
                }
                self.set_error_toast_with_category(toast, category);
                self.push_publish_entry(super::PublishQueueEntry {
                    event_id: signed.id.clone(),
                    kind: signed.unsigned.kind,
                    target_relays: 0,
                    status,
                    relay_outcomes: Vec::new(),
                });
                Vec::new()
            }
        }
    }

    /// Drain every frame the engine pushed into the queue dispatcher since the
    /// last drain, wrap each as a `Content`-lane outbound message, and update
    /// the per-publish queue projection.
    fn drain_publish_engine_frames(&mut self, event_id: &str, kind: u32) -> Vec<OutboundMessage> {
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
            // Empty until the engine settles — T128 fills this via
            // `apply_engine_completions` once the per-relay state machine
            // reaches a terminal verdict.
            relay_outcomes: Vec::new(),
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
        // T128: a terminal ack (Ok or final give-up) may have just settled
        // the publish — apply the terminal verdict to the queue entry before
        // any retry frame drain so the iOS snapshot reflects the new status.
        self.apply_engine_completions();
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
        // T128: `tick` → `dispatch_pending` → synchronous `dispatch_due` may
        // return an OK / failure ack inline. Drain any settled verdicts so
        // the queue entry flips to `"ok"` / `"failed"` on the same tick.
        self.apply_engine_completions();
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

    /// Notify the publish engine that a relay socket is unavailable. Any
    /// in-flight publish for that relay is moved back to durable Pending by
    /// the engine; the actor will retry when a fresh Connected event arrives.
    pub(crate) fn mark_publish_relay_unavailable(&mut self, relay_url: &str) {
        let now_ms = now_epoch_ms();
        if let Err(err) = self
            .publish_engine
            .mark_relay_unavailable(relay_url, now_ms)
        {
            self.publish_engine
                .record_engine_error(&err, &String::new(), "", now_ms);
            let (toast, _, category) = describe_engine_error(&err);
            self.set_error_toast_with_category(toast, category);
        }
    }

    /// Notify the publish engine that a relay socket is available. Pending
    /// publishes targeting this relay are dispatched through the normal actor
    /// outbound path, which also keeps relay-worker connection ownership in
    /// one place.
    pub(crate) fn mark_publish_relay_available(&mut self, relay_url: &str) -> Vec<OutboundMessage> {
        let now_ms = now_epoch_ms();
        if let Err(err) = self.publish_engine.mark_relay_available(relay_url, now_ms) {
            self.publish_engine
                .record_engine_error(&err, &String::new(), "", now_ms);
            let (toast, _, category) = describe_engine_error(&err);
            self.set_error_toast_with_category(toast, category);
            return Vec::new();
        }
        self.apply_engine_completions();
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
            let (toast, _, category) = describe_engine_error(&err);
            self.set_error_toast_with_category(toast, category);
            return Vec::new();
        }
        // T128: resume can complete a publish synchronously when the
        // dispatcher returns OK acks for a re-dispatched retry. Drain
        // terminal verdicts before returning so the boot-resume path
        // surfaces the final status on the same actor frame. (The queue
        // entry for resumed publishes was pushed by the original kernel
        // process — on a fresh kernel B in tests there is no entry to flip;
        // `set_publish_entry_terminal` is a no-op in that case.)
        self.apply_engine_completions();
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
    pub(crate) fn publish_status_snapshot(&self) -> &crate::publish::PublishStatusSnapshot {
        self.publish_engine.snapshot()
    }

    /// Direction review #29: drain ALL terminals that settled since the last
    /// emit, returning them as a JSON array for the `action_results` snapshot
    /// projection. Each tick surfaces every result that arrived, not just the
    /// most recent. The host uses this to resolve any spinner whose
    /// `correlation_id` appears here.
    ///
    /// PR-G — as a sibling effect, every terminal also records an `Accepted`
    /// / `Failed` stage into the `action_stages` snapshot mirror so a host
    /// that listens through the stage seam (a richer lifecycle than the
    /// boolean `action_results` drain) observes the terminal exactly once.
    /// The two surfaces are additive: `action_results` is the per-tick edge,
    /// `action_stages` is the persisted mirror. A host may use either.
    pub(super) fn take_action_results_projection(&mut self) -> serde_json::Value {
        let terminals = self.publish_engine.take_pending_terminals();
        if terminals.is_empty() {
            return serde_json::Value::Null;
        }
        // PR-G: record the terminal into the stage mirror *before* serializing
        // the action_results array. The mirror's `at_ms` is sourced from
        // `now_ms()` so a `FixedClock` keeps the timestamp deterministic.
        let now_ms = self.now_ms();
        for terminal in &terminals {
            let stage = match terminal.status {
                "ok" => super::action_stages::ActionStage::Accepted,
                _ => super::action_stages::ActionStage::Failed {
                    reason: terminal
                        .error
                        .clone()
                        .unwrap_or_else(|| terminal.status.to_string()),
                },
            };
            // `record` is silent on cap hits (D6) — the diagnostic counters
            // surface the event without interrupting the publish path.
            self.action_stages.record(
                &terminal.correlation_id,
                stage,
                None,
                now_ms,
            );
        }
        let arr: Vec<serde_json::Value> = terminals
            .iter()
            .map(|terminal| {
                let status = match terminal.status {
                    "ok" => "published",
                    other => other,
                };
                serde_json::json!({
                    "correlation_id": terminal.correlation_id,
                    "status": status,
                    "error": terminal.error,
                })
            })
            .collect();
        serde_json::Value::Array(arr)
    }

    /// T128: drain every terminal verdict the engine recorded since the last
    /// drain and flip the matching `PublishQueueEntry` from `accepted_locally`
    /// to its terminal `"ok"` / `"failed"` status, carrying the per-relay
    /// outcome map. Called after every engine entrypoint
    /// (`run_publish_engine_at`, `handle_publish_ok_at`, `tick_publish_engine`,
    /// `resume_publish_engine`).
    ///
    /// Status mapping (per the iOS UX requirement — partial success is still
    /// surfaced under the `"ok"` branch with N/M detail):
    /// - `accepted.is_empty() && !failed.is_empty()` → `"failed"`
    /// - any accepted (with or without failures) → `"ok"`
    /// - both empty → `"failed"` defensively (no relays settled at all)
    pub(super) fn apply_engine_completions(&mut self) {
        let completions = self.publish_engine.take_completed();
        if completions.is_empty() {
            return;
        }
        for outcome in completions {
            let (status, outcomes) = classify_terminal_outcome(&outcome);
            self.set_publish_entry_terminal(&outcome.event_id, status, outcomes);
        }
        // `changed_since_emit` is set inside `set_publish_entry_terminal` on
        // any field change; setting again here is redundant but documents the
        // intent (terminal transitions are always snapshot-worthy).
        self.changed_since_emit = true;
    }
}

/// T128: map a `TerminalOutcome` into the wire-level `(status, outcomes)`
/// pair. Kept free-standing so the kernel tests can assert the contract
/// without going through `apply_engine_completions`.
fn classify_terminal_outcome(
    outcome: &TerminalOutcome,
) -> (&'static str, Vec<super::RelayAckOutcome>) {
    let mut outcomes = Vec::with_capacity(outcome.accepted.len() + outcome.failed.len());
    for url in &outcome.accepted {
        outcomes.push(super::RelayAckOutcome {
            relay_url: url.clone(),
            status: "ok".to_string(),
            message: String::new(),
        });
    }
    for (url, reason) in &outcome.failed {
        outcomes.push(super::RelayAckOutcome {
            relay_url: url.clone(),
            status: "failed".to_string(),
            message: reason.clone(),
        });
    }
    let status = if outcome.accepted.is_empty() {
        // Pure failure — every relay reached FailedAfterRetries. (NoTargets
        // never reaches this path; it's handled in `run_publish_engine_at`
        // via `record_engine_error`.)
        "failed"
    } else {
        // At least one Ok — partial-success and full-success both report
        // `"ok"`; the per-relay detail tells iOS whether it's N/M or N/N.
        "ok"
    };
    (status, outcomes)
}
