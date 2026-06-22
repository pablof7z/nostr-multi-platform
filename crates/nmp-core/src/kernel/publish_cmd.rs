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

use super::{is_hex_pubkey, Kernel, OutboundMessage};
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
        self.record_action_stage_coded(
            &correlation_id,
            super::action_stages::ActionStage::Failed {
                reason: error.clone(),
            },
            None,
            reason_code,
            reason_subject.as_deref(),
        );
        self.publish_engine
            .record_action_terminal_failure(correlation_id, error, reason_code);
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
        //
        // `result_json` (ADR-0043 Decision 4) is an opaque structured result
        // body the action attaches to its `action_results` row's `result`
        // field. The kernel never parses it — it only forwards it (D0: no
        // protocol noun enters the substrate).
        self.record_action_stage(
            &correlation_id,
            super::action_stages::ActionStage::Accepted,
            None,
        );
        self.publish_engine
            .record_action_terminal_success(correlation_id, result_json);
        // A terminal verdict is always snapshot-worthy: the next emit drains
        // it into `action_results` via `take_action_results_projection`.
        self.changed_since_emit = true;
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
        // ADR-0055 Rung 1: bump settlement_enqueue_ver (signed_events drain).
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
        // ADR-0055 Rung 1 (F2): drive the drain tristate exactly once per emit
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
        // V5 thin-shell: mirror the transition into the
        // `action_lifecycle` display tracker before persisting to the
        // substrate-level `action_stages` history. Both writes share the
        // same `at_ms` so a TTL eviction in `action_lifecycle` and a
        // history append in `action_stages` are coherent under a
        // `FixedClock`. The mirror takes a `clone` of the stage because
        // `action_stages::record` consumes the value; the display tracker
        // collapses to its own enum independent of substrate growth.
        self.action_lifecycle
            .record_coded(correlation_id, stage.clone(), reason_code, reason_subject, at_ms);
        self.action_stages
            .record(correlation_id, stage, detail, at_ms);
        self.changed_since_emit = true;
        // ADR-0055 Rung 1: bump settlement_enqueue_ver for action_stages/lifecycle.
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
    /// ADR-0055 Rung 3 S1b (§10.4): also drives the `note_copy_emit` Cleared-
    /// edge machine for `action_lifecycle` so that the non-empty → empty
    /// transition (e.g. TTL expiry of the last terminal) parks a `Cleared`
    /// presence in the manifest rather than staying `Unchanged`. This makes
    /// `omit_unchanged`'s inverse pass synthesize an explicit Cleared row,
    /// preventing incremental hosts from retaining the stale lifecycle overlay.
    pub(crate) fn action_lifecycle_projection(&mut self) -> serde_json::Value {
        let now = self.now_ms();
        let len_before = self.action_lifecycle.entry_count();
        let result = self.action_lifecycle.snapshot(now);
        let len_after = self.action_lifecycle.entry_count();
        // ADR-0055 Rung 1 (codex #3): bump ttl_expiry_ver when prune_expired
        // actually removed a row. Wall-clock gated — called from the existing
        // emit/ingest edge (D8 compliant, no separate timer).
        if len_after < len_before {
            self.projection_rev_tracker
                .source_versions
                .bump_ttl_expiry();
        }
        // ADR-0055 Rung 3 S1b (§10.4): drive the Cleared-edge machine once per
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
        let removed_stage = self.action_stages.ack(correlation_id);
        let removed_lifecycle = self.action_lifecycle.dismiss(correlation_id);
        if removed_stage || removed_lifecycle {
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
    /// ADR-0055 Rung 3 S1b (§10.4): also drives the `note_copy_emit`
    /// Cleared-edge machine for `action_stages`. Takes `&mut self` to write
    /// into the `projection_rev_tracker`. The `action_results` drain above
    /// this site (in `projections.rs`) may record a terminal into `action_stages`
    /// within the same tick; this accessor runs after that, so it observes the
    /// final post-drain state.
    pub(crate) fn action_stages_projection(&mut self) -> serde_json::Value {
        let now = self.now_ms();
        let len_before = self.action_stages.entry_count();
        let result = self.action_stages.snapshot(now);
        let len_after = self.action_stages.entry_count();
        if len_after < len_before {
            self.projection_rev_tracker
                .source_versions
                .bump_ttl_expiry();
        }
        // ADR-0055 Rung 3 S1b (§10.4): drive the Cleared-edge machine once per
        // emit. On ack/expiry-of-last-entry the snapshot is Null;
        // was_nonempty=true → note_copy_emit parks Cleared → manifest Cleared
        // → synthesis emits the Cleared row → host drops the stale stage entry.
        self.projection_rev_tracker
            .note_copy_emit("action_stages", !result.is_null());
        result
    }

    /// Hex pubkey of the author of `event_id_hex`, or `None` if that event is
    /// not in the kernel's read-cache.
    ///
    /// Reads `self.events` — the lightweight read-cache — rather than the
    /// store directly. Production ingest (`ingest/timeline.rs`) populates both
    /// in lockstep, so the read-cache is a faithful view; the choice avoids a
    /// store round-trip on the publish hot path. `None` is a normal result
    /// (the event simply hasn't been ingested);
    /// the caller degrades gracefully (D6 — emit the reaction with only the `e`
    /// tag, never panic).
    #[must_use]
    pub(crate) fn event_author(&self, event_id_hex: &str) -> Option<String> {
        self.events.get(event_id_hex).map(|e| e.author.clone())
    }

    /// Latest kind:3 follow set for the active account, distinguishing
    /// "not loaded" from "loaded but empty".
    ///
    /// Returns `Some(pubkeys)` when the active account's kind:3 contact list
    /// IS present in the store — even when no valid `p` tags survive the
    /// hex-validation filter (legitimately empty follow list → `Some(vec![])`).
    ///
    /// Returns `None` when:
    /// - No active account is set, **or**
    /// - The active account's kind:3 has not been ingested yet.
    ///
    /// This is the safety gate for wasm Follow / Unfollow: callers MUST
    /// receive `Some` before editing the follow set. Publishing an edit when
    /// `None` is returned would risk silently wiping an unloaded contact list.
    ///
    /// Note: the list is uncapped — and the follow set is now uncapped
    /// everywhere (#1497 amendment 6 collapsed the follow-feed to one
    /// multi-author interest with no per-author limit).
    #[must_use]
    pub(crate) fn try_current_follows(&self) -> Option<Vec<String>> {
        let (tags, _content) = self.try_current_kind3_event()?;
        let follows = tags
            .iter()
            .filter(|t: &&Vec<String>| t.first().map(String::as_str) == Some("p"))
            .filter_map(|t| t.get(1).cloned())
            .filter(|pk| is_hex_pubkey(pk))
            .collect();
        Some(follows)
    }

    /// Return the active account's FULL existing kind:3 raw event — every tag
    /// verbatim (`Vec<Vec<String>>`, including relay-hint and petname columns
    /// on `p` tags and every non-`p` tag) plus the original `content` string —
    /// so a follow-list edit can splice ONLY the `p` section and re-publish
    /// without discarding the rest of the user's contact list (issue #1246).
    ///
    /// Fails closed: returns `None` when no active account is set OR the active
    /// account's kind:3 has not been ingested yet — the SAME safety gate as
    /// [`Self::try_current_follows`]. Callers MUST receive `Some` before
    /// editing; publishing an edit built from `None` would silently wipe an
    /// unloaded contact list. The tag set is uncapped (a cap is a subscription
    /// concern, not a contact-list-editing one — capping here would silently
    /// drop follows ≥501 on every edit).
    #[must_use]
    pub(crate) fn try_current_kind3_event(&self) -> Option<(Vec<Vec<String>>, String)> {
        let author_hex = self.active_account_pubkey()?;
        let author = crate::kernel::hex_to_pubkey_bytes(author_hex)?;
        let Ok(mut iter) = self.store.scan_by_author_kind(&author, &[3], None, None, 1) else {
            return None;
        };
        let Some(Ok(stored)) = iter.next() else {
            // kind:3 not yet ingested — None, not empty.
            return None;
        };
        Some((stored.raw.tags.clone(), stored.raw.content.clone()))
    }

    /// Resolve the active account's CURRENT kind:3 baseline for a follow-set
    /// edit (the actor `follow` / `follow_many` write path), in priority order:
    ///
    /// 1. The FULL raw kind:3 event from the store — every tag + content
    ///    verbatim ([`Self::try_current_kind3_event`]). This preserves relay
    ///    hints, petnames, non-`p` tags, and content on re-publish (issue
    ///    #1246a). It is the synced / locally-published path.
    /// 2. If NO raw kind:3 is in the store but the capability-owned contacts
    ///    cache KNOWS this account's follow set (`follows()` is `Some`, the empty
    ///    list included), rebuild a minimal `p`-only kind:3 from those follows
    ///    with empty content. The cache is `Some` ONLY when the follow set is
    ///    genuinely known: a brand-new account seeded at `create_account` (empty
    ///    list), an account restored from persisted contacts, or a relay-synced
    ///    cache. In this branch the store — the only place relay hints / petnames
    ///    / non-`p` content ever live — holds no event, so a `p`-only
    ///    reconstruction loses nothing recoverable.
    /// 3. Otherwise `None` — an EXISTING account whose kind:3 has NOT synced yet
    ///    (cache `None`). Editing here would silently clobber an unsynced remote
    ///    contact list, so callers MUST fail closed (issue #1246b).
    ///
    /// This is the gate that distinguishes "no list exists (a brand-new local
    /// account, safe to publish its first kind:3)" from "a list exists remotely
    /// but is not loaded (must fail closed)". The store-only
    /// [`Self::try_current_kind3_event`] remains the wasm reducer seam's gate and
    /// keeps its strict not-loaded → `None` contract unchanged.
    #[must_use]
    pub(crate) fn try_current_kind3_event_for_edit(
        &self,
    ) -> Option<(Vec<Vec<String>>, String)> {
        if let Some(raw) = self.try_current_kind3_event() {
            return Some(raw);
        }
        // No raw kind:3 in the store — fall back to the contacts cache's
        // authoritative follow set. `None` (unknown / unsynced) fails closed;
        // `Some(list)` (known, possibly empty) rebuilds a `p`-only baseline.
        let author_hex = self.active_account_pubkey()?;
        let follows = self.contacts_lookup().follows(author_hex)?;
        let tags = follows
            .into_iter()
            .map(|pk| vec!["p".to_string(), pk])
            .collect();
        Some((tags, String::new()))
    }
}
