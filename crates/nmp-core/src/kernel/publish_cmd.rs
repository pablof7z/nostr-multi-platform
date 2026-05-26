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

use super::{Kernel, OutboundMessage, is_hex_pubkey};
use crate::publish::PublishTarget;
use crate::substrate::SignedEvent;

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
    /// `action_results`. The `PublishNote` dispatch path uses this: the
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
    /// The remote-signer (NIP-46) `PublishNote` path uses this: a parked sign
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
    /// The `nmp_app_dispatch_action` `PublishNote` / `PublishProfile` paths
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
        // A sign-step failure also lifts into the `action_stages`
        // mirror so a host listening only on the stage seam (not the
        // per-tick action_results drain) still sees the `Failed`
        // terminal. The mirror also drives the lifecycle history a
        // diagnostic view would render. The shared `correlation_id` is
        // the join key — the host's stage observer and its
        // action_results observer match on the same value.
        //
        // V5 thin-shell: `record_action_stage` mirrors into both the
        // `action_stages` history AND the `action_lifecycle` display
        // projection in one call, so the host shell sees the terminal
        // appear in `recent_terminal` on the next snapshot tick with no
        // reducer-side bookkeeping.
        self.record_action_stage(
            &correlation_id,
            super::action_stages::ActionStage::Failed {
                reason: error.clone(),
            },
            None,
        );
        self.publish_engine
            .record_action_terminal_failure(correlation_id, error);
        // A terminal verdict is always snapshot-worthy: the next emit drains
        // it into `action_results` via `take_action_results_projection`.
        self.changed_since_emit = true;
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
    /// today (post-V3 the C-ABI symbol `nmp_app_wallet_pay_invoice` is a
    /// thin wrapper that routes through `nmp_app_dispatch_action`'s
    /// `nmp.wallet.pay_invoice` namespace). `None` is reserved for
    /// actor-internal auto-dispatched payments where nothing is waiting on an
    /// id.
    //
    // `#[allow(dead_code)]` was lifted when the
    // `ActorCommand::RecordActionSuccess` dispatch arm landed. The NIP-47
    // wallet response handler is the off-band success consumer for pay-invoice
    // flows, including the NIP-57 LNURL → wallet chain.
    pub fn record_action_success(&mut self, correlation_id: String) {
        // Mirror `record_action_failure`'s dual write: an `Accepted` stage in
        // the `action_stages` mirror so a host listening only on the stage
        // seam sees the terminal, and the per-tick `action_results` drain so
        // the spinner-keyed host clears on the next emit. Same join-key
        // contract — the host's stage observer and its action_results
        // observer match on the same `correlation_id`.
        //
        // V5 thin-shell: `record_action_stage` mirrors into both the
        // `action_stages` history AND the `action_lifecycle` display
        // projection in one call.
        self.record_action_stage(
            &correlation_id,
            super::action_stages::ActionStage::Accepted,
            None,
        );
        self.publish_engine
            .record_action_terminal_success(correlation_id);
        // A terminal verdict is always snapshot-worthy: the next emit drains
        // it into `action_results` via `take_action_results_projection`.
        self.changed_since_emit = true;
    }

    /// Append a lifecycle stage for `correlation_id` to the
    /// `action_stages` projection. Persists until the host acks via
    /// [`Kernel::ack_action_stage`].
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
        let at_ms = self.now_ms();
        // V5 thin-shell: mirror the transition into the
        // `action_lifecycle` display tracker before persisting to the
        // substrate-level `action_stages` history. Both writes share the
        // same `at_ms` so a TTL eviction in `action_lifecycle` and a
        // history append in `action_stages` are coherent under a
        // `FixedClock`. The mirror takes a `clone` of the stage because
        // `action_stages::record` consumes the value; the display tracker
        // collapses to its own enum independent of substrate growth.
        self.action_lifecycle
            .record(correlation_id, stage.clone(), at_ms);
        self.action_stages
            .record(correlation_id, stage, detail, at_ms);
        self.changed_since_emit = true;
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
    pub(crate) fn action_lifecycle_projection(&mut self) -> serde_json::Value {
        let now = self.now_ms();
        self.action_lifecycle.snapshot(now)
    }

    /// Drop the entry for `correlation_id` from the `action_stages`
    /// mirror. Idempotent — an unknown id is a silent no-op (D6).
    /// `changed_since_emit` is set so the next tick re-serialises the now-
    /// reduced mirror.
    pub(crate) fn ack_action_stage(&mut self, correlation_id: &str) {
        if self.action_stages.ack(correlation_id) {
            self.changed_since_emit = true;
        }
    }

    /// Read accessor for [`update`]'s projection emit site. Returns
    /// the full JSON mirror as a copy (NOT a drain): the same `correlation_id`
    /// stays in the snapshot across every tick until the host acks. Returns
    /// `serde_json::Value::Null` when nothing is tracked so the helper can
    /// omit the projection key in steady state.
    pub(crate) fn action_stages_projection(&self) -> serde_json::Value {
        self.action_stages.snapshot()
    }

    /// Hex pubkey of the author of `event_id_hex`, or `None` if that event is
    /// not in the kernel's read-cache.
    ///
    /// Reads `self.events` — the same lightweight read-cache
    /// `reply_tags_for_parent` consults for NIP-10 parent-author re-notification
    /// — rather than the store directly. Production ingest
    /// (`ingest/timeline.rs`) populates both in lockstep, so the read-cache is a
    /// faithful view; the choice keeps reaction-author resolution byte-aligned
    /// with the reply path and avoids a store round-trip on the publish hot
    /// path. `None` is a normal result (the event simply hasn't been ingested);
    /// the caller degrades gracefully (D6 — emit the reaction with only the `e`
    /// tag, never panic).
    #[must_use]
    pub(crate) fn event_author(&self, event_id_hex: &str) -> Option<String> {
        self.events.get(event_id_hex).map(|e| e.author.clone())
    }

    /// Latest kind:3 follow set for `author_hex` (hex pubkeys from `p` tags),
    /// read from the shared store. Empty if no kind:3 is known yet.
    #[must_use]
    pub(crate) fn current_follows(&self, author_hex: &str) -> Vec<String> {
        let Some(author) = crate::kernel::hex_to_pubkey_bytes(author_hex) else {
            return Vec::new();
        };
        let Ok(mut iter) = self
            .store
            .scan_by_author_kind(&author, &[3], None, None, 1)
        else {
            return Vec::new();
        };
        let Some(Ok(stored)) = iter.next() else { return Vec::new(); };
        stored
            .raw
            .tags
            .iter()
            .filter(|t: &&Vec<String>| t.first().map(String::as_str) == Some("p"))
            .filter_map(|t| t.get(1).cloned())
            .filter(|pk| is_hex_pubkey(pk))
            .collect()
    }
}
