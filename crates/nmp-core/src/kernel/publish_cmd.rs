//! Kernel-side publish dispatch — T117 thin shim over `PublishEngine`.
//!
//! Before T117 this file contained a one-shot publish path: resolve NIP-65
//! relays, emit a single `EVENT` frame on `RelayRole::Content`, stamp
//! `accepted_locally`, and forget. The publish-retry FSM
//! (`crate::publish::state`) was dead code (relay-lifecycle review §G5).
//!
//! T117 deletes that pathway and routes every publish through
//! [`Kernel::run_publish_engine`] (`kernel/publish_engine.rs`). The engine:
//!
//! 1. Resolves NIP-65 outbox relays (D3).
//! 2. Drives the per-(event, relay) state machine and pushes per-relay frames
//!    into the kernel's `QueueDispatcher`.
//! 3. Surfaces ack handling, retry policy, AUTH-REQUIRED reauth, and durable
//!    `pending_retries` across kernel restart.
//! 4. Folds inbound `OK` frames back through `Kernel::handle_publish_ok` —
//!    the engine is the single writer of publish state (D4).
//!
//! This file remains the kernel's public `publish_signed` entrypoint so
//! `actor/commands/publish.rs` stays untouched.

use super::{Kernel, OutboundMessage};
use crate::publish::PublishTarget;
use nmp_signer_iface::SignedEvent;

impl Kernel {
    /// Publish a signed event through the publish engine (T117).
    ///
    /// Returns the outbound frames the kernel must send: one per resolved
    /// outbox relay (D3). When the resolver returns no targets the engine
    /// records a `RecentFailure` row and the kernel surfaces a toast (D6) —
    /// the return is `Vec::new()`. The retry / ack / reauth lifecycle is
    /// owned entirely by the engine; the kernel only feeds OK frames in via
    /// `handle_publish_ok` (called from `kernel::ingest::handle_text`).
    pub(crate) fn publish_signed(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine(signed, p_tags, PublishTarget::Auto, None)
    }

    /// [`Kernel::publish_signed`] with an action `correlation_id` to report in
    /// `action_results`. The `PublishRaw` dispatch path uses this: the
    /// host received a registry-minted `correlation_id` before the actor signed
    /// the event, so the publish engine must report that id (not the signed
    /// event's `id`) for the host spinner to be cleared. Every other publish
    /// path (`react`, `follow`, `publish_unsigned_event`, …) uses the plain
    /// [`Kernel::publish_signed`], which reports the event id.
    pub(crate) fn publish_signed_with_correlation(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
        correlation_id_override: Option<String>,
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine(signed, p_tags, PublishTarget::Auto, correlation_id_override)
    }

    /// Publish a signed event to an EXPLICIT relay set — the named D3 opt-out
    /// (`PublishTarget::Explicit`). The verbatim event is routed to exactly
    /// `target`'s relays, bypassing the NIP-65 outbox resolver; everything
    /// else (retry / ack / reauth lifecycle, D6 toast contract) is identical
    /// to [`Kernel::publish_signed`]. `PublishTarget::Auto` callers reach the
    /// resolver unchanged via [`Kernel::publish_signed`]; this sibling exists
    /// so callers can pin kind:445 group messages / kind:1059 gift-wraps to
    /// relays the author's own kind:10002 outbox does not cover.
    pub(crate) fn publish_signed_to(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
        target: PublishTarget,
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine(signed, p_tags, target, None)
    }

    /// [`Kernel::publish_signed_to`] with an action `correlation_id` override.
    /// The remote-signer (NIP-46) `PublishRaw` path uses this: a parked sign
    /// op carries the registry-minted `correlation_id`, and when the broker
    /// turns the request around the idle-tick loop publishes through here so
    /// the engine reports the dispatch `correlation_id` rather than the freshly
    /// signed event's `id`.
    pub(crate) fn publish_signed_to_with_correlation(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
        target: PublishTarget,
        correlation_id_override: Option<String>,
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine(signed, p_tags, target, correlation_id_override)
    }

    /// Record a terminal `"failed"` verdict for a dispatched action whose
    /// publish never reached the engine — the *sign* step failed first.
    ///
    /// The `nmp_app_dispatch_action` `PublishRaw` / `PublishProfile` paths
    /// hand the host a registry-minted `correlation_id` and the host waits to
    /// see its outcome in the `action_results` snapshot projection. Every
    /// other terminal verdict (a queued publish that settles / fails per
    /// relay) reaches `action_results` via the publish engine. A sign-step
    /// failure (no active account, malformed reply id, local-key sign error,
    /// remote-signer timeout / rejection) bypasses the engine entirely — so
    /// without this call the host's spinner keyed on that `correlation_id`
    /// would hang forever (a broken promise: a `correlation_id` was returned but
    /// its outcome is never observable).
    ///
    /// Callers pass `Some(id)` only on a dispatched action that carried a
    /// `correlation_id`; a `react` / `follow` / conformance-harness publish
    /// carries `None` and is a no-op here (nothing is waiting on an id).
    pub fn record_action_failure(&mut self, correlation_id: String, error: String) {
        self.record_action_failure_coded(correlation_id, error, None, None);
    }

    /// As [`Self::record_action_failure`], but attaches the kernel's CURATED
    /// failure `reason_code` (+ optional `reason_subject`) to the
    /// `action_lifecycle` display projection (#1735). The substrate
    /// `action_stages` history and the `action_results` terminal keep only the
    /// English `error` prose — the structured reason code there is S7's (#1754).
    ///
    /// Pass a code ONLY for curated app copy the kernel itself authored (a host
    /// would localize it); leave opaque upstream / executor-supplied diagnostic
    /// text un-coded (`reason_code == None`), mirroring #1711's guard. Shells
    /// localize the code, falling back to the `error` prose.
    pub fn record_action_failure_coded(
        &mut self,
        correlation_id: String,
        error: String,
        reason_code: Option<&'static str>,
        reason_subject: Option<String>,
    ) {
        // S11 slice 2 (#1758): a sign-step failure records its terminal
        // DIRECTLY into the ledger — the single source of `action_results`.
        // `record_terminal` does the dual write in one call: it appends the
        // per-tick `action_results` row (status `"failed"`, the verbatim
        // `error`) AND mirrors the `Failed` stage into the `action_stages`
        // history + the derived `action_lifecycle` view (threading the curated
        // `reason_code` / `reason_subject`, #1735). No second push onto the
        // engine `pending_terminals` — that parallel source is gone; this path
        // never touches the engine. The shared `correlation_id` is the join key.
        //
        // No event was ever signed on this path (the sign step failed), so
        // there is no `event_id` to surface (#1702), and no structured result
        // body. The substrate `action_stages` history sees only the prose
        // `reason`; the structured reason code rides the lifecycle view.
        let at_ms = self.now_ms();
        self.action_ledger.record_terminal(
            &correlation_id,
            super::action_stages::ActionStage::Failed {
                reason: error.clone(),
            },
            "failed",
            Some(error),
            None,
            None,
            reason_code,
            reason_subject.as_deref(),
            at_ms,
        );
        // A terminal verdict is always snapshot-worthy: the next emit drains
        // it into `action_results` via `take_action_results_projection`. Bump
        // the enqueue source version so the `action_stages` / `action_lifecycle`
        // projections (which depend on it) re-serialise this tick.
        self.changed_since_emit = true;
        self.projection_rev_tracker
            .source_versions
            .bump_settlement_enqueue();
    }

    /// Record a terminal `"ok"` verdict for a dispatched action whose terminal
    /// outcome is observed **off-band** from the publish engine — the
    /// action_results-and-action_stages dual surface that
    /// [`Self::record_action_failure`] writes, but for the success leg.
    ///
    /// The motivating consumer is NIP-47 NWC `pay_invoice`: the kind:23194
    /// payment request reaches the publish engine and settles like any other
    /// signed event, but the **payment outcome** arrives separately as the
    /// wallet's kind:23195 response (carrying a `preimage` on success or an
    /// `error` object on failure). The NWC response handler decodes it on the
    /// actor thread and routes here to close the dispatched action's promise
    /// — without this call a host that dispatched `nmp.nip57.zap` would see its
    /// spinner hang forever, exactly the broken-promise gap
    /// `record_action_failure` closes on the failure leg.
    ///
    /// Callers pass `Some(id)` whenever the underlying action carried a
    /// dispatched `correlation_id` — every FFI-originated `pay_invoice` does
    /// today (callers route through `nmp_app_dispatch_action` with namespace
    /// `nmp.wallet.pay_invoice` — the bespoke C-ABI symbol was removed in
    /// #1607). `None` is reserved for
    /// actor-internal auto-dispatched payments where nothing is waiting on an
    /// id.
    //
    // `#[allow(dead_code)]` was lifted when the
    // `ActorCommand::RecordActionSuccess` dispatch arm landed. The NIP-47
    // wallet response handler is the off-band success consumer for pay-invoice
    // flows, including the NIP-57 LNURL → wallet chain.
    pub fn record_action_success(&mut self, correlation_id: String, result_json: Option<String>) {
        // S11 slice 2 (#1758): mirror `record_action_failure`'s single-source
        // write — record the terminal DIRECTLY into the ledger. `record_terminal`
        // appends the per-tick `action_results` row (status `"published"`,
        // carrying `result_json`) AND mirrors the `Accepted` stage into the
        // `action_stages` history + the derived `action_lifecycle` view in one
        // call. No second push onto the engine `pending_terminals` — that
        // parallel source is gone. Same join-key contract — the host's stage
        // observer and its action_results observer match on the same
        // `correlation_id`.
        //
        // `result_json` (ADR-0071 Decision 4) is an opaque structured result
        // body the action attaches to its `action_results` row's `result`
        // field. The kernel never parses it — it only forwards it (D0: no
        // protocol noun enters the substrate). Off-band success (e.g. NWC
        // pay-invoice): the terminal is not a published nostr event, so there
        // is no `event_id` to surface (#1702).
        let at_ms = self.now_ms();
        self.action_ledger.record_terminal(
            &correlation_id,
            super::action_stages::ActionStage::Accepted,
            "published",
            None,
            result_json,
            None,
            None,
            None,
            at_ms,
        );
        // A terminal verdict is always snapshot-worthy: the next emit drains
        // it into `action_results` via `take_action_results_projection`. Bump
        // the enqueue source version so the `action_stages` / `action_lifecycle`
        // projections (which depend on it) re-serialise this tick.
        self.changed_since_emit = true;
        self.projection_rev_tracker
            .source_versions
            .bump_settlement_enqueue();
    }

    /// Record the outcome of a `SignEventForReturn` op under `correlation_id`.
    ///
    /// `Ok(signed_json)` is the standard flat Nostr event JSON the host
    /// attaches to an out-of-band transport; `Err(message)` is a sign failure
    /// (no signer, malformed draft, broker rejection / timeout). Either way the
    /// host's `signEventForReturn` continuation — keyed on `correlation_id` —
    /// resolves on the next snapshot tick. Mirrors `record_action_failure` /
    /// `record_action_success`: the write flips `changed_since_emit` so the
    /// next emit drains the entry into `projections["signed_events"]`.
    ///
    /// Drain-on-emit, not persistent: the host reads each id exactly once.
    /// `take_signed_events_projection` clears the map every tick it produces a
    /// value, so a slow consumer that misses the tick will never see the id
    /// again (the continuation must be registered BEFORE the dispatch — which
    /// the FFI return-then-suspend ordering guarantees).
    pub(crate) fn record_signed_event_return(
        &mut self,
        correlation_id: &str,
        result: Result<String, String>,
    ) {
        self.signed_events
            .insert(correlation_id.to_string(), result);
        self.changed_since_emit = true;
        // ADR-0070 Rung 1: bump settlement_enqueue_ver (signed_events drain).
        self.projection_rev_tracker
            .source_versions
            .bump_settlement_enqueue();
    }

    /// Drain every `SignEventForReturn` result that landed since the last emit
    /// into the `signed_events` snapshot projection, returning a
    /// `correlation_id → { "ok": bool, … }` map. `Null` (→ key omitted) in
    /// steady state, mirroring `take_action_results_projection`.
    ///
    /// Each value is `{ "ok": true, "signed_json": "…" }` on success or
    /// `{ "ok": false, "error": "…" }` on failure — the exact shape the Swift
    /// resolver parses. The map is `clear()`ed here (drain-once), so the host
    /// reads each id exactly once.
    pub(in super::super) fn take_signed_events_projection(&mut self) -> serde_json::Value {
        // ADR-0070 Rung 1 (F2): drive the drain tristate exactly once per emit
        // (mirrors `take_action_results_projection`). Changed on non-empty,
        // Cleared on the non-empty -> empty transition, Unchanged while stably
        // empty.
        let nonempty = !self.signed_events.is_empty();
        self.projection_rev_tracker
            .note_drain_emit("signed_events", nonempty);
        if !nonempty {
            return serde_json::Value::Null;
        }
        let mut out = serde_json::Map::with_capacity(self.signed_events.len());
        for (correlation_id, result) in self.signed_events.drain() {
            let value = match result {
                Ok(signed_json) => serde_json::json!({
                    "ok": true,
                    "signed_json": signed_json,
                }),
                Err(error) => serde_json::json!({
                    "ok": false,
                    "error": error,
                }),
            };
            out.insert(correlation_id, value);
        }
        serde_json::Value::Object(out)
    }

    /// Append a lifecycle stage for `correlation_id` to the
    /// `action_stages` projection. Histories persist until the host acks or the
    /// kernel-owned retention window expires.
    ///
    /// `at_ms` is sourced from the kernel clock (`now_ms`) so a test
    /// `FixedClock` makes the recorded timestamps deterministic. `detail`
    /// is opaque per-stage JSON the host renders verbatim (e.g. relay url
    /// for `Publishing`, error class for `Failed`). The cap behaviour and
    /// drop-oldest eviction live in [`super::action_stages`].
    ///
    /// `changed_since_emit` is set so the next snapshot tick re-serialises
    /// the mirror — same flush convention the rest of the kernel uses for
    /// projection updates.
    pub(crate) fn record_action_stage(
        &mut self,
        correlation_id: &str,
        stage: super::action_stages::ActionStage,
        detail: Option<serde_json::Value>,
    ) {
        self.record_action_stage_coded(correlation_id, stage, detail, None, None);
    }

    /// As [`Self::record_action_stage`], but threads the kernel's curated
    /// failure `reason_code` (+ optional `reason_subject`) into the
    /// `action_lifecycle` display projection ONLY (#1735). The substrate
    /// `action_stages` history sees the un-coded `ActionStage` — its structured
    /// reason code is S7's (#1754). A non-`Failed` stage ignores the code.
    pub(crate) fn record_action_stage_coded(
        &mut self,
        correlation_id: &str,
        stage: super::action_stages::ActionStage,
        detail: Option<serde_json::Value>,
        reason_code: Option<&str>,
        reason_subject: Option<&str>,
    ) {
        let at_ms = self.now_ms();
        // S11 (#1758): one ledger, one write. The ledger owns the substrate
        // stage history AND the curated reason-code sidecar (#1735); the
        // `action_stages` history and the `action_lifecycle`
        // `{in_flight, recent_terminal}` view are BOTH derived from this one
        // record (resolves #1684 — no second tracker). `reason_code` /
        // `reason_subject` ride the derived lifecycle view only; the substrate
        // history keeps just the prose `reason`.
        self.action_ledger.record_coded(
            correlation_id,
            stage,
            detail,
            reason_code,
            reason_subject,
            at_ms,
        );
        self.changed_since_emit = true;
        // ADR-0070 Rung 1: bump settlement_enqueue_ver for action_stages/lifecycle.
        self.projection_rev_tracker
            .source_versions
            .bump_settlement_enqueue();
    }

    /// Read accessor for the `action_lifecycle` display projection
    /// (V5 thin-shell). Returns the host-facing
    /// `{in_flight, recent_terminal}` payload or
    /// [`serde_json::Value::Null`] when nothing is tracked.
    ///
    /// TTL pruning runs inside the tracker's `snapshot` so a quiet
    /// kernel still drops expired terminals on the next emit. `now_ms`
    /// routes through the kernel clock so a `FixedClock` keeps tests
    /// deterministic.
    ///
    /// ADR-0070 Rung 3 S1b (§10.4): also drives the `note_copy_emit` Cleared-
    /// edge machine for `action_lifecycle` so that the non-empty → empty
    /// transition (e.g. TTL expiry of the last terminal) parks a `Cleared`
    /// presence in the manifest rather than staying `Unchanged`. This makes
    /// `omit_unchanged`'s inverse pass synthesize an explicit Cleared row,
    /// preventing incremental hosts from retaining the stale lifecycle overlay.
    pub(crate) fn action_lifecycle_projection(&mut self) -> serde_json::Value {
        let now = self.now_ms();
        let len_before = self.action_ledger.lifecycle_entry_count();
        // S11 (#1758): derived from the ONE ledger, not a parallel tracker.
        let result = self.action_ledger.lifecycle_snapshot(now);
        let len_after = self.action_ledger.lifecycle_entry_count();
        // ADR-0070 Rung 1 (codex #3): bump ttl_expiry_ver when prune_expired
        // actually removed a row. Wall-clock gated — called from the existing
        // emit/ingest edge (D8 compliant, no separate timer).
        if len_after < len_before {
            self.projection_rev_tracker
                .source_versions
                .bump_ttl_expiry();
        }
        // ADR-0070 Rung 3 S1b (§10.4): drive the Cleared-edge machine once per
        // emit. `note_copy_emit` parks `Cleared` in `pending_presence` on the
        // was_nonempty && !nonempty edge so the manifest flips to Cleared and
        // the synthesis in `omit_unchanged` emits the host-facing Cleared row.
        // Must be called AFTER the TTL bump above (the bump may advance the rev,
        // which is the Cleared frame's distinguishing rev increment).
        self.projection_rev_tracker
            .note_copy_emit("action_lifecycle", !result.is_null());
        result
    }

    /// Early-dismiss `correlation_id` from the retained action feedback
    /// trackers. Idempotent — an unknown id is a silent no-op (D6).
    ///
    /// An ack is a genuine content change to one or both retained projections:
    /// the reduced mirror/lifecycle serialises to different bytes. So besides
    /// flipping `changed_since_emit` (so the actor ticks) we MUST advance the
    /// projection rev — otherwise the StaleStamp oracle fires (content changed,
    /// rev didn't) and, with Rung 3 omit-Unchanged live, the host would serve
    /// stale retained rows. We bump `settlement_enqueue_ver` — the same source
    /// version `record_action_stage` bumps on enqueue; both `action_stages` and
    /// `action_lifecycle` depend on it (see PROJECTION_DEPS in
    /// `projection_rev/mod.rs`), and an ack is the symmetric content edit to an
    /// enqueue.
    ///
    /// NOTE: this is distinct from the `note_copy_emit` Cleared edge. The Cleared
    /// edge (bumps `ttl_expiry_ver`) fires only on ack/expiry of the LAST entry
    /// (non-empty → empty). A PARTIAL ack (entries remain) keeps the projection
    /// non-empty and is governed entirely by this `settlement_enqueue_ver` bump
    /// → delivered as `Changed` exactly once. Both mechanisms are required
    /// (#1390 review FIX 2).
    pub(crate) fn ack_action_stage(&mut self, correlation_id: &str) {
        // S11 (#1758): one ack against the one ledger. The lifecycle view is
        // derived, so dismissing the ledger row drops it from BOTH the
        // `action_stages` history and the `action_lifecycle` projection.
        let removed = self.action_ledger.ack(correlation_id);
        if removed {
            self.changed_since_emit = true;
            // Advance the rev so a partial ack is delivered as Changed exactly
            // once and the oracle stays sharp without relying on a presence
            // override.
            self.projection_rev_tracker
                .source_versions
                .bump_settlement_enqueue();
        }
    }

    /// Read accessor for [`update`]'s projection emit site. Returns
    /// the full JSON mirror as a copy (NOT a drain): entries stay in the
    /// snapshot across ticks until the host acks or the retention window
    /// expires. Returns `serde_json::Value::Null` when nothing is tracked so the
    /// helper can omit the projection key in steady state.
    ///
    /// ADR-0070 Rung 3 S1b (§10.4): also drives the `note_copy_emit`
    /// Cleared-edge machine for `action_stages`. Takes `&mut self` to write
    /// into the `projection_rev_tracker`. The `action_results` drain above
    /// this site (in `projections.rs`) may record a terminal into `action_stages`
    /// within the same tick; this accessor runs after that, so it observes the
    /// final post-drain state.
    pub(crate) fn action_stages_projection(&mut self) -> serde_json::Value {
        let now = self.now_ms();
        let len_before = self.action_ledger.stages_entry_count();
        let result = self.action_ledger.stages_snapshot(now);
        let len_after = self.action_ledger.stages_entry_count();
        if len_after < len_before {
            self.projection_rev_tracker
                .source_versions
                .bump_ttl_expiry();
        }
        // ADR-0070 Rung 3 S1b (§10.4): drive the Cleared-edge machine once per
        // emit. On ack/expiry-of-last-entry the snapshot is Null;
        // was_nonempty=true → note_copy_emit parks Cleared → manifest Cleared
        // → synthesis emits the Cleared row → host drops the stale stage entry.
        self.projection_rev_tracker
            .note_copy_emit("action_stages", !result.is_null());
        result
    }
}
