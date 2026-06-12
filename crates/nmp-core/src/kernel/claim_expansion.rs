//! W5 — per-claim Phase 1/2/3 state machine controller.
//!
//! # Overview
//!
//! Each call to `Kernel::register_claim_expansion` creates a `PendingClaim`
//! that tracks the originating interest, author, deadline, attempted relay
//! set, and Phase-2 candidate queue. Three public entry points on `impl Kernel`
//! drive the state machine:
//!
//! - `register_claim_expansion` — called from `requests/event.rs::claim_event`
//!   after the existing OneshotApi registration (§7.3 retarget).
//! - `poll_claim_expansion(now)` — called from the actor idle tick (W6). Checks
//!   Phase-1 budget, applies the §8.7 preflight, promotes Phase-1 claims past
//!   their budget to Phase 2, and marks the total-budget elapse as Terminal.
//! - `on_claim_outcome_hit / on_claim_outcome_eose_no_match` — called from the
//!   ingest seam (W3) when an EVENT or EOSE is observed for a claim sub.
//!
//! # Doctrine compliance
//!
//! - **D0** — all types use substrate-pure `(Pubkey, RelayUrl, String)`.
//! - **D4** — `&mut self` methods are the sole writers of `pending_claims`
//!   and `claim_sub_index`.
//! - **D6** — no `Result` returns; unknown sub_ids / duplicate registrations
//!   are silent no-ops; the relay-tried cap terminates deterministically.
//! - **D8** — `poll_claim_expansion` is O(active_claims); the cap check is
//!   a length-0 early exit when no claims are pending.
//!
//! # §8.2 — Phase 2 via hints, not per-candidate interests
//!
//! Phase 2 mutates the existing `LogicalInterest`'s `hints` vec (re-pushed
//! via `registry.push()`) rather than creating new interests per candidate.
//! This keeps `oneshot.in_flight()` at 1/claim and avoids contending with
//! `MAX_DISCOVERY_CONCURRENCY`.
//!
//! # §8.3 — Twin BTreeMaps
//!
//! `pending_claims: BTreeMap<InterestId, PendingClaim>` stores per-claim state.
//! `claim_sub_index: BTreeMap<String, InterestId>` maps wire sub_ids → claims
//! for O(log N) ingest lookups. The reverse index is populated by
//! `register_planner_wire_frames` (bridge in `kernel/requests/mod.rs`).

use std::collections::{BTreeSet, VecDeque};
use crate::time::Instant;

use crate::planner::{InterestId, InterestShape};
use crate::relay::CanonicalRelayUrl;

use super::{relay_score::ClaimOutcome, wire_log, Kernel, OutboundMessage};

// ── Constants ────────────────────────────────────────────────────────────────
// These mirror the values in `relay_score.rs` (§0 Q3).

/// Phase-1 budget before expansion begins.
pub(super) const PHASE_1_BUDGET_MS: u64 = 1500;
/// Maximum concurrency for Phase-2 hint candidates.
pub(super) const MAX_EXPANSION_CONCURRENCY: usize = 3;
/// Hard cap on total relays tried per claim (Phase 1 + Phase 2).
pub(super) const MAX_RELAYS_TRIED_PER_CLAIM: usize = 12;
/// Total per-claim wall-clock budget. After this, terminate regardless.
pub(super) const PER_CLAIM_TOTAL_BUDGET_MS: u64 = 8000;

// ── Phase state ──────────────────────────────────────────────────────────────

/// Current phase of a pending claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for Phase-1 (warm outbox relays) EOSE/EVENT.
    Phase1,
    /// Phase-2 expansion in flight; hints have been pushed.
    Phase2InFlight,
    /// Terminated — see [`ClaimTermination`].
    Terminal(ClaimTermination),
}

/// Why a claim terminated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimTermination {
    /// An EVENT matching the claim was received.
    Hit,
    /// All candidate relays were tried; none delivered the event.
    Exhausted,
    /// The per-claim total budget elapsed before any hit.
    Budget,
}

// ── PendingClaim ─────────────────────────────────────────────────────────────

/// Per-claim bookkeeping for the Phase 1/2/3 state machine.
///
/// Stored in `Kernel.pending_claims: BTreeMap<InterestId, PendingClaim>`.
pub(crate) struct PendingClaim {
    /// `InterestId` of the OneshotApi interest registered for this claim.
    pub(crate) interest_id: InterestId,
    /// The event's `primary_id` (hex event-id or `"kind:pubkey:d_tag"` coord).
    pub(crate) primary_id: String,
    /// Optional claim author. Present for naddr URIs and nevent URIs with
    /// author TLV; `None` for nevent without author TLV (§7.3).
    pub(crate) author: Option<String>,
    /// The interest shape — needed to rebuild the `LogicalInterest` when
    /// pushing Phase-2 hints via `registry.push()`.
    pub(crate) shape: InterestShape,
    /// Wall-clock instant at which this claim was registered.
    pub(crate) started_at: Instant,
    /// Current phase of this claim.
    pub(crate) phase: Phase,
    /// Set of relay URLs already attempted (Phase 1 + Phase 2).
    /// URLs stored in canonical form (via `CanonicalRelayUrl::parse_or_raw`).
    /// Used for dedup and for the §8.1 `relay_failed` walk.
    pub(crate) attempted: BTreeSet<String>,
    /// Ordered queue of Phase-2 relay candidates (relay URLs, desc score).
    pub(crate) candidate_queue: VecDeque<String>,
    /// (canonical_relay_url, sub_id) pairs currently in-flight for Phase 2.
    ///
    /// B4 fix: sub_ids are shape-only (wire.rs:153 derives from
    /// `canonical_filter_hash`, not relay URL). Multiple relays share the
    /// same sub_id for identical filter shapes. The tuple key lets the EOSE
    /// handler attribute outcomes per-relay independently.
    ///
    /// Populated by `register_planner_wire_frames` bridge.
    pub(crate) in_flight_attempts: BTreeSet<(String, String)>,
}

impl PendingClaim {
    fn new(
        interest_id: InterestId,
        primary_id: String,
        author: Option<String>,
        shape: InterestShape,
        initial_hints: Vec<String>,
        started_at: Instant,
    ) -> Self {
        // Build initial candidate queue from provided hints (URI relay hints
        // from the nevent TLV per §7.3).
        let candidate_queue: VecDeque<String> = initial_hints.into_iter().collect();
        Self {
            interest_id,
            primary_id,
            author,
            shape,
            started_at,
            phase: Phase::Phase1,
            attempted: BTreeSet::new(),
            candidate_queue,
            in_flight_attempts: BTreeSet::new(),
        }
    }

    fn elapsed_ms(&self, now: Instant) -> u64 {
        now.duration_since(self.started_at)
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

// ── impl Kernel ──────────────────────────────────────────────────────────────

impl Kernel {
    /// Register a new claim-expansion tracker.
    ///
    /// Called from `requests/event.rs::claim_event` after the OneshotApi
    /// registration. `interest_id` is the id assigned by `OneshotApi::request`.
    /// `primary_id` is the event-id or coordinate string. `author` is `Some`
    /// for naddr and for nevent with author TLV, `None` otherwise. `uri_hints`
    /// are relay URLs from the URI's NIP-19 relay TLV (fed as Phase-1
    /// candidates). `started_at` is the wall-clock instant of registration.
    ///
    /// D6: duplicate registrations (same `primary_id` already pending) are
    /// silent no-ops.
    pub(crate) fn register_claim_expansion(
        &mut self,
        primary_id: String,
        interest_id: Option<InterestId>,
        author: Option<String>,
        uri_hints: Vec<String>,
        started_at: Instant,
    ) {
        // Resolve the interest_id — use the provided value or the most-recently
        // registered one. If neither is available, skip (claim_event must have
        // registered the interest before calling us).
        let iid = match interest_id {
            Some(id) => id,
            None => {
                // Find the interest registered for this primary_id by looking at
                // the last-inserted oneshot token entry. Use a synthetic id based
                // on the shape hash when none is explicitly provided (test path).
                // In production, `claim_event` always passes the real interest_id.
                // For tests that call without an interest_id, use a stable sentinel.
                InterestId(0)
            }
        };

        // D6: duplicate primary_id → no-op
        if self
            .pending_claims
            .values()
            .any(|c| c.primary_id == primary_id)
        {
            return;
        }

        // Determine the InterestShape from what claim_event registered.
        // For tests (iid == 0) we use a minimal shape; production calls provide
        // the real iid and shape comes from the registry.
        let shape = self
            .lifecycle
            .registry_mut()
            .iter_active()
            .into_iter()
            .find(|i| i.id == iid)
            .map(|i| i.shape.clone())
            .unwrap_or_else(InterestShape::default);

        let claim = PendingClaim::new(
            iid.clone(),
            primary_id.clone(),
            author.clone(),
            shape,
            uri_hints,
            started_at,
        );

        self.pending_claims.insert(iid, claim);

        wire_log::log_wire(wire_log::WireLogEvent::ClaimPhaseAdvance {
            author: author.as_deref().unwrap_or(""),
            from: "none",
            to: "phase1",
            reason: "registered",
        });
    }

    /// Advance the claim state machine for all pending claims.
    ///
    /// Called from the actor idle tick (W6) with the current `Instant::now()`.
    ///
    /// For each claim:
    /// 1. §8.7 preflight: if the event is already known, mark Terminal(Hit).
    /// 2. If Phase1 budget elapsed AND total budget not elapsed → Phase2.
    /// 3. If total budget elapsed → Terminal(Budget).
    ///
    /// Returns empty `Vec` (OutboundMessages come from the planner compile
    /// triggered by `CompileTrigger::ViewOpened` when hints are updated).
    ///
    /// D8: O(active_claims), early exit when empty.
    pub(crate) fn poll_claim_expansion(&mut self, now: Instant) -> Vec<OutboundMessage> {
        if self.pending_claims.is_empty() {
            return Vec::new();
        }

        // Collect claim actions to apply after iterating (borrow-checker safety).
        let mut to_terminate: Vec<(InterestId, ClaimTermination)> = Vec::new();
        let mut to_advance_to_phase2: Vec<InterestId> = Vec::new();

        for (iid, claim) in &self.pending_claims {
            let elapsed = claim.elapsed_ms(now);

            // §8.7 preflight: event already in store → terminate as Hit
            if self.event_already_known(&claim.primary_id) {
                to_terminate.push((iid.clone(), ClaimTermination::Hit));
                continue;
            }

            // Total budget expired → terminate as Budget
            if elapsed >= PER_CLAIM_TOTAL_BUDGET_MS {
                to_terminate.push((iid.clone(), ClaimTermination::Budget));
                continue;
            }

            match &claim.phase {
                Phase::Phase1 => {
                    if elapsed >= PHASE_1_BUDGET_MS {
                        to_advance_to_phase2.push(iid.clone());
                    }
                }
                Phase::Phase2InFlight => {
                    // Phase-2 slot management: fill open slots from candidate queue.
                    // in_flight_attempts is a (relay, sub_id) set; count unique
                    // relay dimensions for the concurrency limit.
                    let in_flight_relay_count = claim
                        .in_flight_attempts
                        .iter()
                        .map(|(relay, _)| relay.as_str())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len();
                    let open_slots =
                        MAX_EXPANSION_CONCURRENCY.saturating_sub(in_flight_relay_count);
                    if open_slots > 0
                        && !claim.candidate_queue.is_empty()
                        && claim.attempted.len() < MAX_RELAYS_TRIED_PER_CLAIM
                    {
                        to_advance_to_phase2.push(iid.clone());
                    }
                    // If no candidates remain and no in-flight attempts → exhausted
                    if claim.candidate_queue.is_empty() && claim.in_flight_attempts.is_empty() {
                        to_terminate.push((iid.clone(), ClaimTermination::Exhausted));
                    }
                }
                Phase::Terminal(_) => {
                    // Already terminal — will be pruned below
                }
            }
        }

        // Apply Phase-2 promotions
        for iid in to_advance_to_phase2 {
            self.advance_to_phase2(iid, now);
        }

        // Apply terminations
        for (iid, reason) in to_terminate {
            self.terminate_claim(iid, reason);
        }

        // Prune terminal entries
        self.pending_claims
            .retain(|_, c| !matches!(c.phase, Phase::Terminal(_)));

        Vec::new()
    }

    /// Handle a matching EVENT on a claim-expansion sub.
    ///
    /// Called by the W3 ingest seam when an accepted EVENT arrives on a
    /// subscription identified by `sub_id`. Marks the claim terminal and cleans
    /// up the reverse index (B3: claim_sub_index must not accumulate).
    pub(crate) fn on_claim_outcome_hit(&mut self, sub_id: &str) {
        // O(log N) lookup via the reverse index (B4: sub_id alone names shape,
        // not relay; still unambiguous for InterestId lookup).
        let Some(iid) = self.claim_sub_index.get(sub_id).cloned() else {
            // Fallback: scan by sub_id prefix — handles test-injected claims
            // that never went through register_planner_wire_frames.
            let maybe_iid = self
                .pending_claims
                .values()
                .find(|c| c.in_flight_attempts.iter().any(|(_, s)| s == sub_id))
                .map(|c| c.interest_id.clone());
            if let Some(iid) = maybe_iid {
                self.terminate_claim(iid.clone(), ClaimTermination::Hit);
                // B3: clean up reverse index entries for this claim
                self.claim_sub_index.retain(|_, v| *v != iid);
                self.pending_claims
                    .retain(|_, c| !matches!(c.phase, Phase::Terminal(_)));
            }
            return;
        };
        self.terminate_claim(iid.clone(), ClaimTermination::Hit);
        // B3: clean up all reverse-index entries that pointed to this claim
        self.claim_sub_index.retain(|_, v| *v != iid);
        self.pending_claims
            .retain(|_, c| !matches!(c.phase, Phase::Terminal(_)));
    }

    /// Handle a matching EVENT on a claim-expansion sub identified by `primary_id`.
    ///
    /// Legacy entry point used by some call sites that have the primary_id
    /// but not the sub_id. Routes through the primary-id scan path.
    pub(crate) fn on_claim_outcome_hit_by_primary_id(&mut self, primary_id: &str) {
        let Some(iid) = self
            .pending_claims
            .values()
            .find(|c| c.primary_id == primary_id)
            .map(|c| c.interest_id.clone())
        else {
            return;
        };
        self.terminate_claim(iid.clone(), ClaimTermination::Hit);
        // B3: clean up all reverse-index entries that pointed to this claim
        self.claim_sub_index.retain(|_, v| *v != iid);
        self.pending_claims
            .retain(|_, c| !matches!(c.phase, Phase::Terminal(_)));
    }

    /// Handle an EOSE-without-match on a claim-expansion sub.
    ///
    /// Called by the W3 ingest seam when an EOSE arrives on a sub_id that
    /// belongs to this claim and no matching EVENT was seen. Frees the
    /// in-flight slot (keyed by the `(relay_url, sub_id)` tuple — B4) and
    /// records the relay as attempted.
    pub(crate) fn on_claim_outcome_eose_no_match(&mut self, sub_id: &str, relay_url: &str) {
        let Some(iid) = self.claim_sub_index.get(sub_id).cloned() else {
            return;
        };
        let Some(claim) = self.pending_claims.get_mut(&iid) else {
            return;
        };

        // B4: remove the (relay, sub_id) pair, not just sub_id. Multiple
        // relays share the same sub_id for the same filter shape; removing
        // only sub_id would falsely mark all relays done when the first
        // one EOSEs.
        let canonical_relay = CanonicalRelayUrl::parse_or_raw(relay_url).into_string();
        claim
            .in_flight_attempts
            .remove(&(canonical_relay.clone(), sub_id.to_string()));
        claim.attempted.insert(canonical_relay.clone());

        wire_log::log_wire(wire_log::WireLogEvent::EoseRx {
            sub_id,
            relay_url: &canonical_relay,
            matched: false,
        });
    }

    /// Walk pending claims and record a `Failed` outcome for any claim that
    /// attempted the given relay URL. Called from `relay_lifecycle.rs::relay_failed`.
    pub(crate) fn relay_failed_claim_walk(&mut self, relay_url: &str) {
        let canonical = CanonicalRelayUrl::parse_or_raw(relay_url);
        let canonical_str = canonical.as_str().to_string();

        // Collect (author, relay) pairs to score — can't call record_claim_outcome
        // inside the mutable borrow of pending_claims.
        let to_score: Vec<(String, String)> = self
            .pending_claims
            .values()
            .filter_map(|claim| {
                if claim.attempted.contains(&canonical_str) {
                    claim.author.clone().map(|a| (a, canonical_str.clone()))
                } else {
                    None
                }
            })
            .collect();

        for (author, url) in to_score {
            self.record_claim_outcome(&author, &url, ClaimOutcome::Failed);
        }
    }

    /// Release a claim (user navigated away). Removes the claim from tracking
    /// and cleans up the reverse index (B3: no claim_sub_index accumulation).
    ///
    /// Called from `release_event` or equivalent in `requests/event.rs`.
    pub(crate) fn release_claim_expansion(&mut self, primary_id: &str) {
        let maybe_iid = self
            .pending_claims
            .values()
            .find(|c| c.primary_id == primary_id)
            .map(|c| c.interest_id.clone());
        if let Some(iid) = maybe_iid {
            // B3: remove all reverse-index entries that pointed to this claim
            self.claim_sub_index.retain(|_, v| *v != iid);
            self.pending_claims.remove(&iid);
        }
    }

    /// V-59 rung 1 (#4) — resolve the `primary_id` of a claim whose oneshot
    /// `sub_id` just EOSE'd WITHOUT a match.
    ///
    /// Returns `Some(primary_id)` only when ALL of:
    /// - `sub_id` maps to a live claim (`claim_sub_index` → `pending_claims`),
    ///   which means no matching EVENT terminated it (a hit removes the entry
    ///   via `on_claim_outcome_hit`), and
    /// - the event is not already in the store (`!event_already_known`) — a
    ///   late duplicate or a hit recorded on a sibling relay would otherwise
    ///   make the EOSE a non-event.
    ///
    /// Read-only: the caller (`complete_unknown_oneshot`) performs the state
    /// teardown so the single-writer discipline stays at one site.
    pub(in crate::kernel) fn claim_primary_id_for_unmatched_sub(
        &self,
        sub_id: &str,
    ) -> Option<String> {
        let iid = self.claim_sub_index.get(sub_id)?;
        let claim = self.pending_claims.get(iid)?;
        if self.event_already_known(&claim.primary_id) {
            return None;
        }
        Some(claim.primary_id.clone())
    }

    /// Looks up the author for a claim-expansion sub from the twin BTreeMaps.
    ///
    /// W5 replacement for the W3 stub in `relay_score_record.rs::lookup_claim_expansion_author`.
    pub(crate) fn lookup_claim_author_by_sub_id(&self, sub_id: &str) -> Option<String> {
        let iid = self.claim_sub_index.get(sub_id)?;
        let claim = self.pending_claims.get(iid)?;
        claim.author.clone()
    }
}
