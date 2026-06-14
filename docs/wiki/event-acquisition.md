---
title: Event Acquisition
slug: event-acquisition
topic: event-acquisition
summary: "There is a single event-acquisition mechanism: serving from the local store is its first half, the planner's wire REQ is its refinement half, running through th"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Event Acquisition

## Mechanism

There is a single event-acquisition mechanism: serving from the local store is its first half, the planner's wire REQ is its refinement half, running through the same seam with no domain-specific stages.

Persisted WatermarkRow.synced_up_to has zero production writers; the live since-floor is derived from store content (newest matching event via query_visit), making the issue's conservative-floor premise false. (Previously: sync.md claimed WatermarkRow '[LANDED M4]', but it was deleted and replaced by the presence heuristic without re-deciding the soundness question; then the heuristic itself was superseded by direct store-content derivation.)

The `shape_to_store_queries` mapping is the single source for both the floor computation and the serve decision (ADR-0045 §6); no second shape→query mapping exists.

Locally published events route through the same `ingest_…` + `notify_event_observers` fan-out the relay arm uses, providing read-your-writes for follows and giving local echo to kind:0 and DM self-copies. Follow/unfollow local publish feeds this same observer fan-out, restoring read-your-writes (Previously: locally published follow/unfollow events skipped observer fan-out, so the follow-list projection and ActiveFollowSet never updated until app restart, account switch, or a different kind:3 arrived; then PR #1199 landed the read-your-writes fix). (Previously: routing was described without the read-your-writes-on-exact-surface rationale and the PR #1199 landing.)

The ActiveFollowSet/FollowListProjection producer is merged into one fold with two views, and the 500-cap divergence between kernel and observer is fixed.

Un-floored NEG-OPEN reconciles the full un-watermarked window so set reconciliation can repair gaps below the heuristic floor.

The address-pointer watermark branch aligns with the V-118 min/abort rule instead of taking max over coords and ignoring coords with zero stored events.

Every EVENT arriving on ingest records a delivery Hit (sampled or saturating) to feed the relay score map, activating the W4 warm filter.

Permanent relay errors (401/403) enter a Denied{until} long-backoff state instead of killing the worker thread, and ensure_open respawns exited slots.

Publishing to AUTH-requiring relays parks the publish via the existing availability-gate seam and re-dispatches on `Authenticated`, instead of failing after one-shot reauth within a 250ms tick.

NEG-OPEN liveness has a wall-clock deadline via the existing actor tick infrastructure, preventing silent starvation when a relay ignores the frame.

Eviction below an active coverage floor lowers that floor in the same transaction, preventing permanent holes under the watermark.

A switching-cost term computed against the prior plan damps greedy relay-selection churn during incremental NIP-65 arrivals.

Coverage_dropped_authors appears on CompiledPlan symmetrically with unroutable_authors, surfacing authors dropped at budget exhaustion.

ingest_repost uses max(existing_slot_ts, wrapper_ts) so an older repost wrapper cannot pull a root downward.

load_older grows one page at a time, clamps to a ceiling (window_limit: AtomicUsize initialized to 80, max 500), and returns false at the limit.

Timeline fan-out clamps created_at to cached.created_at.min(now_secs), while StoredEvent retains the wire timestamp for protocol correctness.

The cold-start route now calls request_probe for every tagged pubkey, closing the probe→cache→recompile loop so DM-inbox discovery fires (the Case C bootstrap-inbox probe fix).

Greedy-merge determinism uses a total canonical sort key (not just the spec's tuple key) to break ties consistently for REQ output, making it input-order-independent.

Rule 1 wildcard-kinds × concrete-kinds merges are refused (mirroring Rule 9) to close an all-kinds privacy/bandwidth leak.

The planner PR was split to keep only the two genuinely-safe fixes (DM-inbox-discovery bug and merge determinism); Defect 2 (T129 watermark rewrite) and Defect 4 (wildcard absorption) were reverted as documented, tested features, and escalated as owner decisions #1281/#1282.

The `gc_step` budgeting uses a resumable Phase-1 cursor, O(1) Phase-2 count, and hourly Phase-3 tombstone gate, with the LRU event-count ceiling (`HOT_EVENT_CEILING`) disabled until store-claims are wired.

<!-- citations: [^da6b1-82] [^da6b1-15] [^2e544-8] [^2e544-9] [^2e544-10] [^4277-4280] [^3720-3721] [^2e544-11] [^2e544-12] [^2e544-13] [^2e544-14] [^2e544-15] [^2e544-16] [^2e544-17] [^2e544-18] [^2e544-19] [^1278-2238-2267] [^1278-2249-2255] [^1278-2253-2254] [^1280-2386-2436] [^1280-2396-2436] [^02745-7] [^2e544-55] [^02745-55] [^02745-99] [^2e544-349] [^2e544-369] [^2e544-463] -->
