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
    apply_selection, CompiledPlan, InterestId, InterestLifecycle, InterestShape, MailboxCache,
    PlannerError, SubscriptionCompiler,
};
use crate::stable_hash::stable_hash64;

use super::trigger::CompileTrigger;
use super::wire::{plan_diff, WireFrame};
use super::{SubscriptionLifecycle, MAILBOX_PROBE_BATCH};

impl SubscriptionLifecycle {
    /// Recompile from current registry + caller-supplied mailbox state, diff
    /// against the last-compiled plan, and return the `WireFrame` delta.
    ///
    /// T132: the mailbox cache is no longer owned by the lifecycle. The kernel
    /// passes its `KernelMailboxes` adapter (a view onto `author_relay_lists`,
    /// populated by `ingest_relay_list` from real kind:10002 events); tests
    /// pass a local `InMemoryMailboxCache`. This eliminates the dual-source
    /// hazard the planner-side cache previously created.
    ///
    /// Updates the lifecycle gate; diverts REQs targeting auth-paused relays
    /// into the pending-auth buffer.
    pub fn recompile_and_diff(
        &mut self,
        mailbox_cache: &dyn MailboxCache,
    ) -> Result<Vec<WireFrame>, PlannerError> {
        let interests = self.registry.iter_active();
        let compiler = SubscriptionCompiler::with_relays(
            mailbox_cache,
            &self.indexer_relays,
            &self.active_account_read_relays,
            &self.app_relays,
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
        // `docs/perf/codex-reviews/076173d.md`).
        apply_selection(
            &mut plan,
            self.select_max_connections,
            self.select_max_per_user,
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
        if let Some(wm) = self.watermark_fn.as_ref() {
            apply_watermark_rewrite(&mut plan, wm.as_ref());
        }

        let prior = self.current_plan.as_ref();
        let raw_frames = plan_diff(prior, Some(&plan), &interests);

        self.current_plan = Some(plan);

        let mut frames = self.auth_gate.partition(raw_frames);

        // Implicit kind:10002 discovery (D3). Any author this REQ targets
        // whose mailbox is neither cached NOR previously probed gets an
        // auto-emitted `kinds:[10002]` REQ to the indexer set. The relay's
        // answer lands in the kernel's mailbox cache via `ingest_relay_list`,
        // which fires `Nip65Arrived` → the next recompile routes the author
        // through their declared write relays. Authors who never published a
        // kind:10002 are probed exactly once (the empty EOSE still marks them
        // probed) so we don't re-REQ every recompile.
        //
        // These frames are auxiliary: they are NOT part of `CompiledPlan`,
        // do NOT affect `plan_id`, and are appended AFTER the auth partition
        // (the indexer is not an auth-paused relay). v1 scope: `shape.authors`
        // only — `#p` tag values and address-pointer pubkeys are a
        // documented follow-up.
        if !self.indexer_relays.is_empty() {
            let mut to_probe: BTreeSet<String> = BTreeSet::new();
            for interest in &interests {
                for author in &interest.shape.authors {
                    if self.probed_mailboxes.contains(author) {
                        continue;
                    }
                    if mailbox_cache.get(author).is_some() {
                        continue;
                    }
                    to_probe.insert(author.clone());
                }
            }
            if !to_probe.is_empty() {
                let batch: Vec<String> = to_probe.iter().cloned().collect();
                for chunk in batch.chunks(MAILBOX_PROBE_BATCH) {
                    let sub_id = format!(
                        "mailbox-probe-{:08x}",
                        stable_hash64(("mailbox-probe", chunk)) & 0xFFFF_FFFF
                    );
                    let filter_json = serde_json::json!({
                        "kinds": [10002],
                        "authors": chunk,
                        "limit": chunk.len(),
                    })
                    .to_string();
                    for indexer in &self.indexer_relays {
                        frames.push(WireFrame::Req {
                            relay_url: indexer.clone(),
                            sub_id: sub_id.clone(),
                            filter_json: filter_json.clone(),
                            interest_id: InterestId(u64::MAX),
                            lifecycle: InterestLifecycle::OneShot,
                        });
                    }
                }
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
    #[must_use]
    pub fn drain_tick(&mut self, mailbox_cache: &dyn MailboxCache) -> Vec<WireFrame> {
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
        match self.recompile_and_diff(mailbox_cache) {
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

// ─── T129 watermark rewrite ──────────────────────────────────────────────────

/// Returns `true` when every kind in `shape.kinds` is in the ephemeral range
/// 20000..30000 (per NIP-01 §3 ephemerals). Empty `kinds` is "wildcard" and
/// is NOT considered ephemeral — persistent kinds may match, so the rewrite
/// still applies. Mirrors the carve-out NDK added in commit `5afbd245`.
pub(super) fn shape_is_ephemeral_only(shape: &InterestShape) -> bool {
    !shape.kinds.is_empty() && shape.kinds.iter().all(|k| (20000..30000).contains(k))
}

/// In-place rewrite of every non-ephemeral sub-shape's `since` to
/// `max(existing_since, watermark + 1)`.
///
/// The rewrite is purely a value mutation — `canonical_filter_hash` is left
/// untouched so the wire-emitter's diff treats a re-opened sub as the same
/// `sub_id` it had before (the watermark moves between recompiles, but the
/// REQ is only emitted on the first compile that introduces the shape).
/// This matches NDK's `opts.addSinceFromCache` once-at-sub-open semantics
/// (`core/src/subscription/index.ts:537`).
///
/// D8: walks the plan tree exactly once; no per-shape allocation beyond the
/// one closure call into the resolver (which itself is responsible for
/// reusing its index buffers via `query_visit(limit=1)`).
pub(super) fn apply_watermark_rewrite(
    plan: &mut CompiledPlan,
    watermark_fn: &(dyn Fn(&InterestShape) -> Option<u64> + Send + Sync),
) {
    for relay_plan in plan.per_relay.values_mut() {
        for sub_shape in &mut relay_plan.sub_shapes {
            if shape_is_ephemeral_only(&sub_shape.shape) {
                continue;
            }
            let Some(watermark) = watermark_fn(&sub_shape.shape) else {
                continue;
            };
            let floor = watermark.saturating_add(1);
            sub_shape.shape.since = Some(match sub_shape.shape.since {
                Some(existing) if existing >= floor => existing,
                _ => floor,
            });
        }
    }
}
