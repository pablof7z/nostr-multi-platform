//! Recompile / drain-tick core: the planner-invocation seam.
//!
//! Split out of `subs/mod.rs` (file-size-gate, NMP #169) with zero
//! behavioural change. Holds [`SubscriptionLifecycle::recompile_and_diff`],
//! [`SubscriptionLifecycle::drain_tick`], and the T129 watermark-rewrite free
//! functions they depend on. `SubscriptionLifecycle`'s struct definition (and
//! thus the privacy boundary) lives in the module root; this is a sibling
//! child module of `subs`, so the private fields remain reachable here.

use std::collections::BTreeSet;

use crate::planner::{
    apply_selection_with_lookup, InterestId, InterestLifecycle, MailboxCache, PlannerError,
    SubscriptionCompiler,
};
use crate::stable_hash::stable_hash64;
use nmp_planner::RelayAuthorScoreLookup;

use super::trigger::CompileTrigger;
use super::watermark_rewrite::apply_watermark_rewrite;
use super::wire::{plan_diff, WireFrame};
use super::{SubscriptionLifecycle, MAILBOX_PROBE_BATCH};

impl SubscriptionLifecycle {
    /// Recompile from current registry + caller-supplied mailbox state, diff
    /// against the last-compiled plan, and return the `WireFrame` delta.
    ///
    /// T132: the mailbox cache is no longer owned by the lifecycle. The kernel
    /// passes its `KernelMailboxes` adapter over the substrate `MailboxCache`
    /// populated by the registered kind:10002 parser; tests pass a local
    /// `InMemoryMailboxCache`. This eliminates the dual-source hazard the
    /// planner-side cache previously created.
    ///
    /// Updates the lifecycle gate; diverts REQs targeting auth-paused relays
    /// into the pending-auth buffer.
    ///
    /// Equivalent to `recompile_and_diff_with_lookup(mailbox_cache, None)`.
    /// Use [`Self::recompile_and_diff_with_lookup`] to supply a warm-relay
    /// score filter (W4).
    pub fn recompile_and_diff(
        &mut self,
        mailbox_cache: &dyn MailboxCache,
    ) -> Result<Vec<WireFrame>, PlannerError> {
        self.recompile_and_diff_with_lookup(mailbox_cache, None)
    }

    /// Recompile with an optional W4 warm-relay score filter.
    ///
    /// W4: `score_lookup` is the optional warm-relay filter. The kernel passes
    /// `Some(lookup)` (via `ScoreLookupRef` built from `relay_score_map`) so
    /// the planner's greedy step sees only warm outbox relays for authors that
    /// have at least one warm option. Call sites that do not need W4 should use
    /// the default-arity [`Self::recompile_and_diff`] wrapper.
    ///
    /// Updates the lifecycle gate; diverts REQs targeting auth-paused relays
    /// into the pending-auth buffer.
    pub fn recompile_and_diff_with_lookup(
        &mut self,
        mailbox_cache: &dyn MailboxCache,
        score_lookup: Option<&dyn RelayAuthorScoreLookup>,
    ) -> Result<Vec<WireFrame>, PlannerError> {
        self.recompile_inner(mailbox_cache, score_lookup, None)
    }

    /// Recompile with a W4 warm-relay score filter AND a blocked-relay set.
    ///
    /// Like [`Self::recompile_and_diff_with_lookup`] but additionally:
    /// - Captures the pre-block attribution snapshot into
    ///   `current_plan_attribution` (SPLIT A: diagnostic purpose).
    /// - Removes blocked relays from the wire-authoritative plan after capture
    ///   (SPLIT B: wire-safety — blocked relays must not receive REQs).
    ///
    /// Called by `drain_tick_with_lookup_and_blocked` (the actor idle-loop
    /// bridge path in `lifecycle_drain.rs`).
    pub fn recompile_and_diff_with_blocked(
        &mut self,
        mailbox_cache: &dyn MailboxCache,
        score_lookup: Option<&dyn RelayAuthorScoreLookup>,
        blocked: &crate::substrate::BlockedRelaySet,
    ) -> Result<Vec<WireFrame>, PlannerError> {
        self.recompile_inner(mailbox_cache, score_lookup, Some(blocked))
    }

    /// Core recompile implementation. All public recompile entry points delegate
    /// here. `blocked` controls the SPLIT A/B attribution-capture + block-filter.
    fn recompile_inner(
        &mut self,
        mailbox_cache: &dyn MailboxCache,
        score_lookup: Option<&dyn RelayAuthorScoreLookup>,
        blocked: Option<&crate::substrate::BlockedRelaySet>,
    ) -> Result<Vec<WireFrame>, PlannerError> {
        let interests = self.registry.iter_active();

        // ── Compile-input memoization ───────────────────────────────────────
        //
        // Computing the O(authors × relays) plan is expensive. If every
        // compile input is identical to the last run, the output plan is
        // deterministic and the wire diff is empty — skip the compiler.
        //
        // Fingerprint covers:
        //  • active interest set (shapes + ids)            — LogicalInterest: Hash
        //  • mailbox_generation                            — bumped with mailbox triggers
        //    (NOTE: mailbox_cache.generation() is NOT used here because
        //    KernelMailboxes::generation() always returns 0; enqueue_trigger()
        //    bumps this lifecycle counter for NIP-65 mailbox triggers instead)
        //  • dead-relay set                                — BTreeSet<String>
        //  • all relay URL lists (indexer, account-read,
        //    app, bootstrap-content, bootstrap-indexer)    — Vec<String>
        //  • selection budget (max_connections, max_per_user)
        //  • watermark_generation                          — bumped at EOSE/NEG-DONE
        //    (CRITICAL: missing this causes stale `since` → silent under-fetch)
        //  • blocked relay set (GAP-2 fix)                 — sorted URL iterator
        //    Without this, a kind:10006-only change leaves the fingerprint
        //    unchanged → memo guard returns the cached plan → SPLIT B never
        //    re-runs → the blocked relay keeps its REQ.
        //
        // Score-lookup (W4) is intentionally excluded: the score map only
        // influences `apply_selection`, not `compile()`, and score cells
        // change on live claim activity — including them would defeat the
        // optimisation on every claim tick. A stale score selection is at
        // worst sub-optimal routing for one tick, not a correctness issue.
        //
        // The coverage hook (`coverage_hook`) is excluded and the memo is
        // DISABLED when a hook is installed, because a hook may carry
        // interior mutability (`Arc<Mutex<bool>>`) that changes its behaviour
        // without any observable state change on this struct. Only pure hooks
        // are safe to memo — and we cannot verify purity here. The production
        // hook (negentropy coverage filter) is installed at startup and then
        // left constant, so the guard below conservatively skips the memo for
        // any hook-installed lifecycle.
        let skip_memo = self.coverage_hook.is_some();

        let fingerprint = if skip_memo {
            // Sentinel: no fingerprint caching when a coverage hook is present.
            0u64
        } else {
            use std::hash::Hash;
            let mut h = crate::stable_hash::StableHasher::new();
            interests.hash(&mut h);
            self.mailbox_generation.hash(&mut h);
            self.dead_relays.hash(&mut h);
            self.indexer_relays.hash(&mut h);
            self.active_account_read_relays.hash(&mut h);
            self.app_relays.hash(&mut h);
            self.bootstrap_content_relays.hash(&mut h);
            self.bootstrap_indexer_relays.hash(&mut h);
            self.select_max_connections.hash(&mut h);
            self.select_max_per_user.hash(&mut h);
            self.watermark_generation.hash(&mut h);
            // GAP-2: include the blocked set so a kind:10006-only change
            // (block/unblock) forces a real recompile and SPLIT B re-runs.
            if let Some(blocked) = blocked {
                for url in blocked.iter() {
                    url.hash(&mut h);
                }
            }
            h.finish64()
        };

        if !skip_memo && self.last_compile_fingerprint == Some(fingerprint) {
            // Inputs unchanged — the plan is identical to last compile; the
            // wire diff is empty. Return without invoking the compiler.
            return Ok(Vec::new());
        }

        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            mailbox_cache,
            &self.indexer_relays,
            &self.active_account_read_relays,
            &self.app_relays,
            &self.bootstrap_content_relays,
            &self.bootstrap_indexer_relays,
        );
        let mut plan = compiler.compile(&interests)?;
        self.compile_count = self.compile_count.saturating_add(1);

        // Health filter: strip relays the actor has marked dead BEFORE the
        // selector runs. The selector's candidate set is then the alive
        // subset, so authors with a dead-only declared write set lose any
        // landing pad and the selector retires them into "uncovered" (they
        // simply don't appear in any surviving sub_shape). Authors with
        // mixed alive/dead declared write relays naturally pick the alive
        // ones during coverage rounds.
        //
        // Doing this BEFORE compile would shrink the plan_id input set;
        // doing it AFTER apply_selection would leave dead relays in the
        // wire diff. Between the two is the right seam.
        if !self.dead_relays.is_empty() {
            plan.per_relay
                .retain(|url, _| !self.dead_relays.contains(url));
        }

        // Greedy max-coverage selection — applesauce-style. The naive plan
        // connects to every NIP-65 write relay declared by every follow
        // (in real data: hundreds). This pass reduces the relay set to
        // ≤ `select_max_connections` with a per-author redundancy cap of
        // `select_max_per_user`. Runs BEFORE the coverage hook / watermark
        // so both downstream passes see only the surviving (relay, shape)
        // set. `apply_selection` mutates each affected `SubShape` in place
        // and calls `recompute_hash()` so the wire-emitter's diff produces
        // the correct REQ/CLOSE delta. Plan-id is intentionally NOT
        // recomputed (see `planner/mod.rs` §"Plan-id determinism vs.
        // post-compile mutators"; M4 precedent in
        // `docs/retired/removed-documents.md`).
        apply_selection_with_lookup(
            &mut plan,
            self.select_max_connections,
            self.select_max_per_user,
            score_lookup,
        );

        // D2 negentropy-first: let the coverage-gate hook (M4) rewrite the
        // plan before the wire-emitter sees it — skipping authoritative
        // (filter, relay) pairs and bumping `since` on pairs we already have
        // a watermark for. With no hook installed (the kernel-only path) the
        // plan flows through unchanged.
        if let Some(hook) = self.coverage_hook.as_ref() {
            hook(&mut plan);
        }

        // T129 — addSinceFromCache: rewrite each non-ephemeral shape's
        // `since` to `max(existing_since, watermark + 1)` so a freshly-opened
        // REQ does not re-fetch events the cache already has. Runs AFTER the
        // coverage hook so the two passes compose monotonically: coverage may
        // bump `since`, the watermark rewrite then raises it further if the
        // store has even fresher events. We intentionally do NOT recompute
        // `canonical_filter_hash` here — sub_id stability is the feature
        // (`planner/mod.rs::canonical_filter_hash` docs the rationale).
        //
        // The interests slice is forwarded so apply_watermark_rewrite can
        // resolve each sub-shape's lifecycle: Tailing since=None is narrowed
        // (live feed, skip already-cached events); non-Tailing since=None
        // stays None (backfill/oneshot, full history requested — #1281 intent).
        if let Some(wm) = self.watermark_fn.as_ref() {
            apply_watermark_rewrite(&mut plan, wm.as_ref(), &interests);
        }

        // SPLIT A — diagnostic attribution snapshot (pre-block, post-selection).
        //
        // Captured here — after greedy selection and watermark rewrite, before
        // the blocked-relay post-pass — so the diagnostics projection can report
        // "would-be" attribution for all selected relays, including those that
        // are currently blocked. The snapshot is read by
        // `Kernel::relay_diagnostics_snapshot` via `current_plan_attribution()`.
        self.current_plan_attribution = plan
            .per_relay
            .iter()
            .map(|(url, relay_plan)| (url.clone(), relay_plan.attribution.clone()))
            .collect();

        // SPLIT B — block filter: remove blocked relays from the
        // wire-authoritative plan. Blocked relays must not receive REQs (the
        // user's kind:10006 list is a signal-to-noise / privacy boundary).
        // The diagnostic snapshot above already captured attribution for these
        // relays, so the shell can still surface them (it derives a hue from the
        // raw `"blocked"` connection token).
        if let Some(blocked) = blocked {
            if !blocked.is_empty() {
                plan.per_relay.retain(|url, _| !blocked.contains(url));
            }
        }

        let prior = self.current_plan.as_ref();
        let raw_frames = plan_diff(prior, Some(&plan), &interests);

        self.current_plan = Some(plan);
        self.last_compile_fingerprint = Some(fingerprint);

        let mut frames = self.auth_gate.partition(raw_frames);

        // Implicit kind:10002 discovery (D3). Any author this REQ targets
        // whose mailbox is neither cached NOR previously probed gets an
        // auto-emitted `kinds:[10002]` REQ. The relay's answer lands in the
        // kernel's mailbox cache via the registered kind:10002 parser, which
        // fires `Nip65Arrived` → the next recompile routes the author through
        // their declared write relays. Authors who never published a kind:10002
        // are probed exactly once (the empty EOSE still marks them probed) so
        // we don't re-REQ every recompile.
        //
        // Probe target = `indexer_relays ∪ app_relays`. The probe is additive
        // to app relays for the same reason the Case A kind:0/discovery lane is
        // (`case_a_authors.rs:145-156`;
        // `docs/design/subscription-compilation/outbox.md:153-158`): kind:10002
        // is a plain replaceable event that any general relay serves, and the
        // dedicated-indexer set can be empty (operator opted out) or AUTH-walled.
        // Without the app-relay
        // union, a Chirp install whose only indexer-role relay is AUTH-walled
        // could never fetch third-party NIP-65 lists → the outbox model would
        // silently stall. Unioning app relays keeps discovery working when an
        // app-owned content relay serves kind:10002 anonymously.
        //
        // These frames are auxiliary: they are NOT part of `CompiledPlan` and
        // do NOT affect `plan_id`. They ARE routed through the auth gate (see
        // below): now that the probe can target an app relay (which, unlike the
        // dedicated indexer, may be AUTH-walled), a probe REQ to a paused relay
        // must be buffered and flushed on `Authenticated` exactly like a content
        // REQ — never sent blind to a relay that will reject it.
        //
        // v1 scope: `shape.authors` only — `#p` tag values and address-pointer
        // pubkeys are a documented follow-up.
        //
        // #3132 — an author already targeted by a registered interest that
        // directly carries `kinds:[10002, ...]` (e.g. the bootstrap self-kinds
        // tailing interest, `startup.rs::SELF_KINDS_TAILING`) is already
        // getting its own kind:10002 fetch through the compiled plan; probing
        // it again here duplicates that REQ (the exact symptom filed in
        // #3132: `{kinds:[10002],authors:[X],limit:1}` alongside
        // `{kinds:[0,3,10002,...],authors:[X]}` for the same author). Skip
        // those authors — the compiled interest's own REQ is the mailbox
        // discovery.
        let mut kind10002_covered: BTreeSet<String> = BTreeSet::new();
        for interest in &interests {
            if interest.shape.kinds.contains(&crate::kinds::KIND_RELAY_LIST) {
                kind10002_covered.extend(interest.shape.authors.iter().cloned());
            }
        }

        let mut probe_relays: BTreeSet<String> = BTreeSet::new();
        probe_relays.extend(self.indexer_relays.iter().cloned());
        probe_relays.extend(self.app_relays.iter().cloned());
        if !probe_relays.is_empty() {
            let mut to_probe: BTreeSet<String> = BTreeSet::new();
            for interest in &interests {
                for author in &interest.shape.authors {
                    if self.probed_mailboxes.contains(author) {
                        continue;
                    }
                    if mailbox_cache.get(author).is_some() {
                        continue;
                    }
                    if kind10002_covered.contains(author) {
                        continue;
                    }
                    to_probe.insert(author.clone());
                }
            }
            if !to_probe.is_empty() {
                let batch: Vec<String> = to_probe.iter().cloned().collect();
                let mut probe_frames: Vec<WireFrame> = Vec::new();
                for chunk in batch.chunks(MAILBOX_PROBE_BATCH) {
                    let sub_id = format!(
                        "mailbox-probe-{:08x}",
                        stable_hash64(("mailbox-probe", chunk)) & 0xFFFF_FFFF
                    );
                    let filter_json = serde_json::json!({
                        "kinds": [crate::kinds::KIND_RELAY_LIST],
                        "authors": chunk,
                        "limit": chunk.len(),
                    })
                    .to_string();
                    for relay in &probe_relays {
                        probe_frames.push(WireFrame::Req {
                            relay_url: relay.clone(),
                            sub_id: sub_id.clone(),
                            filter_json: filter_json.clone(),
                            interest_id: InterestId(u64::MAX),
                            lifecycle: InterestLifecycle::OneShot,
                        });
                    }
                }
                // Auth-gate the probes: paused-relay probes are buffered (and
                // flushed on `Authenticated`), failed-relay probes are dropped
                // fail-closed, live-relay probes pass through.
                frames.extend(self.auth_gate.partition(probe_frames));
                self.probed_mailboxes.extend(to_probe);
            }
        }

        Ok(frames)
    }

    /// Drain the trigger inbox at a tick boundary. Per D8, all triggers
    /// collapse into at most one compile pass; an empty inbox is a no-op.
    ///
    /// T132: the caller supplies the mailbox cache for the same reason
    /// [`Self::recompile_and_diff`] does — the lifecycle is no longer the
    /// owner of mailbox state.
    ///
    /// T140 (D6 / codex finding #7): this path is FFI-visible (driven by the
    /// actor idle loop via `Kernel::drain_lifecycle_tick`). The previous
    /// `recompile_and_diff(...).unwrap_or_default()` silently discarded every
    /// planner error — a D6 violation. We now classify the `Err`:
    /// `EmptyInterestSet` is a benign steady state (no interests registered →
    /// empty diff, common between account switches) and yields an empty `Vec`
    /// without recording; genuine structural errors (`InvalidShape`,
    /// `HashingFailed`) are surfaced into `last_planner_error` (observable via
    /// [`Self::last_planner_error`]) before returning empty, so the error is
    /// never silently lost.
    ///
    /// Equivalent to `drain_tick_with_lookup(mailbox_cache, None)`. Use
    /// [`Self::drain_tick_with_lookup`] to supply a W4 warm-relay score filter.
    #[must_use]
    pub fn drain_tick(&mut self, mailbox_cache: &dyn MailboxCache) -> Vec<WireFrame> {
        self.drain_tick_with_lookup(mailbox_cache, None)
    }

    /// Drain the trigger inbox with an optional W4 warm-relay score filter.
    ///
    /// W4: `score_lookup` threads through to `recompile_and_diff_with_lookup`
    /// so the warm-relay pre-filter is applied on every drain tick. The kernel
    /// passes `Some(lookup)` (via `ScoreLookupRef`); tests and non-W4 paths
    /// should use the default-arity [`Self::drain_tick`] wrapper.
    #[must_use]
    pub fn drain_tick_with_lookup(
        &mut self,
        mailbox_cache: &dyn MailboxCache,
        score_lookup: Option<&dyn RelayAuthorScoreLookup>,
    ) -> Vec<WireFrame> {
        self.drain_tick_inner(mailbox_cache, score_lookup, None)
    }

    /// Drain the trigger inbox with a W4 warm-relay score filter AND a
    /// blocked-relay set.
    ///
    /// Called by `Kernel::drain_lifecycle_tick` (the actor idle-loop bridge)
    /// so the kernel's `snapshot_blocked_relays()` is applied on every drain.
    /// Passes `blocked` into `recompile_and_diff_with_blocked` (SPLIT A+B).
    #[must_use]
    pub fn drain_tick_with_lookup_and_blocked(
        &mut self,
        mailbox_cache: &dyn MailboxCache,
        score_lookup: Option<&dyn RelayAuthorScoreLookup>,
        blocked: &crate::substrate::BlockedRelaySet,
    ) -> Vec<WireFrame> {
        self.drain_tick_inner(mailbox_cache, score_lookup, Some(blocked))
    }

    /// Core drain-tick implementation. Public entry points delegate here.
    fn drain_tick_inner(
        &mut self,
        mailbox_cache: &dyn MailboxCache,
        score_lookup: Option<&dyn RelayAuthorScoreLookup>,
        blocked: Option<&crate::substrate::BlockedRelaySet>,
    ) -> Vec<WireFrame> {
        let triggers = self.inbox.drain_coalesced();
        if triggers.is_empty() {
            return Vec::new();
        }
        // Apply auth-state transitions before recompile so the gate's pause
        // predicate is current when `partition` runs inside `recompile_and_diff`.
        // On `Authenticated`, `record_transition` also returns any REQs that
        // were buffered while the relay was paused; collect them so they are
        // returned alongside the recompile diff. The `plan_diff` inside
        // `recompile_and_diff` does NOT re-emit those frames (the plan is
        // unchanged — only auth state changed), so we must extend here.
        // Production auth flushes go through `handle_auth_state_change` (direct
        // path in `ingest/auth_handlers.rs`), so this path is exercise-only via
        // tests and future callers; correctness here prevents silent drops.
        let mut auth_flushed: Vec<WireFrame> = Vec::new();
        for t in &triggers {
            if let CompileTrigger::RelayAuthStateChanged { url, state } = t {
                auth_flushed.extend(self.auth_gate.record_transition(url.clone(), state.clone()));
            }
        }
        match self.recompile_inner(mailbox_cache, score_lookup, blocked) {
            Ok(mut frames) => {
                frames.extend(auth_flushed);
                frames
            }
            // Benign: no interests registered (e.g. between account switches).
            // Not an error condition — empty diff, nothing to surface.
            Err(PlannerError::EmptyInterestSet) => auth_flushed,
            // D6: a genuine structural planner error must be observable, never
            // swallowed. Record it; the diff is empty for this tick.
            Err(e) => {
                self.last_planner_error = Some(e.to_string());
                auth_flushed
            }
        }
    }
}

// T129 watermark-rewrite helpers are in `super::watermark_rewrite`
// (extracted to satisfy the 500-LOC file-size gate — AGENTS.md).
