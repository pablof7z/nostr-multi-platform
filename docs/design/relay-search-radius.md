# Relay-Search-Radius Expansion for OneshotApi Event Fetches

**Status**: Accepted product behavior for event `resolve_ref` relay-search expansion.
Implementation anchors live in `crates/nmp-core/src/kernel/claim_expansion*.rs`,
`crates/nmp-core/src/kernel/relay_score*.rs`, and the
`relay_search_radius_*` integration tests in `crates/nmp-testing/tests/`.

**Scope**: The OneshotApi event-fetch path that backs `nmp_app_resolve_ref(namespace=1, key, …)` (the renderer's "I have an event key, get me the event" entry point; `key` is a 64-hex event-id, `"kind:pubkey:d"` naddr coordinate, or `"i:<external-id>"` NIP-73 ref — not a `nostr:` URI). All other OneshotApi shapes (profile claims, thread hydration) are explicitly out-of-scope for this iteration — see §11.

**Doctrines**: D0 (substrate purity in `nmp-core`), D4 (`InterestRegistry` is the single writer), D6 (no panics across FFI), D8 (no polling — every score update is edge-triggered).

This document is the durable product/design contract. Implementation plans and
review notes are temporal artifacts and must not be committed as reference docs.

---

## 1. Problem statement

When the renderer triggers `nmp_app_resolve_ref(namespace=1, key, …)` for an embedded event
(e.g. the naddr coordinate of a `nostr:naddr1…` article in note content), the warm request path starts
with:

1. The configured `app_relays` (operator-pinned, additive — protected against selector pruning in [`680666a0`](https://github.com/pablof7z/nostr-multi-platform/commit/680666a0)).
2. The author's NIP-65 outbox relays, **capped at `max_per_user = 2`** by the greedy max-coverage selector in `crates/nmp-planner/src/selection.rs`.

If the event isn't on those ~2–3 relays, the renderer sees indefinite "loading" chrome forever. The other 10+ relays the author published to are never queried.

**Worked example (the canonical regression)**

Gigi's article *"What's left of the internet?"* (kind:30023, `d="the-internet-left-me"`). Her NIP-65 declares 13 write relays. With `purplepag.es` as the sole app_relay:

- The selector picks two of her outbox relays (e.g. `atlas.nostr.land + eden.nostr.land`).
- The article isn't on either. Both EOSE. Renderer stuck.
- The other 11 relays — including `wss://relay.dergigi.com` which has the event — are never queried.

The user has no recovery path short of an operator manually adding another relay.

---

## 2. Goals and non-goals

### Goals (in scope for this feature)

- **G1.** Resolve `nmp_app_resolve_ref(namespace=1, …)` for an event whose author published to ≥1 relay we can reach, even if that relay is not in our app_relays and not in the selector's `max_per_user` picks.
- **G2.** Learn over time which `(author, relay)` pairs actually deliver events, so steady-state claims still hit the right relays first (no perpetual expansion cost).
- **G3.** Survive kernel restart: scores reload from durable storage and bias the next session's Phase 1 choices.
- **G4.** Stay D0/D4/D6/D8-clean. The score table is generic `(Pubkey, RelayUrl, Score)` — no protocol noun — and the kernel actor is the single writer.

### Non-goals (explicit)

- **N1.** Replacing the greedy max-coverage selector for *non-claim* sub-shapes (the steady-state follow-list firehose, profile hydration, NIP-65 fetches). Those keep using `apply_selection` with its existing `max_per_user` cap.
- **N2.** UI/UX for "still searching…" vs. "exhausted" states in iOS/Compose. Tracked as a follow-up; the spec only guarantees the kernel exposes the state.
- **N3.** Cross-author score sharing ("relay X serves the long tail well overall"). Scores are strictly per `(author, relay)` — preserves the invariant that a relay can be great for Gigi and useless for Alice.
- **N4.** Active outbox sweeping / pre-warming. The score table is *passively* populated by the claim path; we do not eagerly walk NIP-65 outboxes on startup.

---

## 3. Acceptance criteria

Numbered for traceability into the relay-search-radius integration tests.

**A1.** From a cold gallery TUI launched against `app_relays = [purplepag.es]` only — no Primal, no Gigi-relay — claiming Gigi's article (`naddr1…the-internet-left-me`) resolves within ~5 s, verified by reading `NMP_CLAIM_LOG`. The claim log must show:

1. An initial REQ to `purplepag.es` (Phase 1).
2. A subsequent REQ to ≥1 NIP-65 outbox relay of Gigi's not in app_relays (Phase 2).
3. An EVENT frame from whichever Phase 2 relay actually serves the event, followed by oneshot completion.

**A2.** A subsequent claim for any other event authored by Gigi prefers the relay that succeeded last time, i.e. that relay appears in the Phase 1 REQ set, not deferred to Phase 2.

**A3.** Scoring survives kernel restart. A Phase-1-warm-path test that fully shuts down the kernel, reopens against the same store, and re-claims a Gigi event must hit Gigi's known-good relay in Phase 1 without any Phase 2 expansion.

**A4.** All doctrine lints green: `cargo test -p nmp-testing --test doctrine_lint_smoke`.

**A5.** A relay that becomes unreachable mid-claim does not stall the search — Phase 2 advances to the next candidate within the wall-clock budget defined in §6.

**A6.** Two simultaneous claims for distinct events by the same author share one expansion budget if the registry deduplicates their shapes (it will not, in practice, since `event_ids` differ — but the scoring updates from claim A's outcome must be visible to claim B before B's Phase 1 set is computed if B's interest registers strictly after A's first score update).

---

## 4. Three-phase behaviour

The behaviour is broken into **three phases per claim**, plus a
**persistence/learning loop** that bridges across claims.

```text
Claim arrives → Phase 1 (warm) → [event found?] → done
                              ↘ [EOSE everywhere / budget elapsed?] → Phase 2 (expansion)
                                                                  → Phase 3 (score writeback)
```

### 4.1 Phase 1 — warm REQ

**Trigger**: A new OneshotApi event-fetch request lands (`nmp_app_resolve_ref(namespace=1, …)` path; specifically the shape is `InterestShape { event_ids: {…} }` or `InterestShape { authors: {…}, kinds: {…}, d_tags: {…} }` for addressable events). The triggering call site is `Kernel::pending_view_requests` → planner compile cycle.

**Relay set**: union of

- All `app_relays` (operator-pinned; never pruned by the selector — invariant from `680666a0`).
- The author's NIP-65 outbox, **filtered to "preferred"**:
  - Already-connected (no socket-open cost), **OR**
- Score ≥ `WARM_THRESHOLD` (0.40; source of truth:
  `crates/nmp-core/src/kernel/relay_score.rs`).
- If the author has no NIP-65 yet (we haven't seen kind:10002), fall back to `app_relays` only and pre-emptively kick off a NIP-65 fetch for the author. (This already happens via `collect_unknown_refs` indirectly; the spec just notes the dependency.)

**Outcome that advances the phase**:

- **Hit** (EVENT frame matches the claim's filter): the OneshotApi token completes via `complete_unknown_oneshot`. **Score update**: increment `(author, R)` for the relay R that delivered. **Stop.** Other Phase 1 relays may still EOSE; their entries get a *neutral* `seen-but-didn't-deliver` outcome — neither incremented nor decremented (the event might genuinely not be on R; we can't tell whether R is bad or just doesn't have this specific event).
- **EOSE-without-match from every Phase 1 relay**: advance to Phase 2.
  EOSE-without-match is neutral for scoring; it records recency but does not
  increment success or failure counters.
- **Phase 1 wall-clock budget elapsed**
  (`PHASE_1_BUDGET_MS = 1500`): advance to Phase 2 even if some Phase 1 relays
  are still pending. Pending relays continue running in parallel; their later
  outcomes still feed the score table.

**Edge cases handled in Phase 1**

- *Phase 1 relay accepts the REQ but never EOSE's.* D8 forbids polling. We use the actor's existing wall-clock-gated observer (the one driving `drain_lifecycle_tick`) to fire a `PhaseTimeout` event when the Phase 1 budget elapses. No `sleep` loop.
- *Phase 1 relay declared unreachable* (`Kernel::relay_failed`): the relay
  leaves the Phase 1 set; if the set is now empty before the budget elapses,
  advance to Phase 2 immediately. Score update: `ClaimOutcome::Failed`.
- *No NIP-65 known, no app_relays configured*: claim immediately fails (cannot search; surfaced via `claim_state = exhausted`). Operator configuration error.

### 4.2 Phase 2 — radius expansion

**Trigger**: Phase 1 advanced as described above.

**Relay set**: the author's NIP-65 outbox **minus** the Phase 1 set. We do not re-query Phase 1 relays in Phase 2 — they've already EOSE'd or are still running.

**Ordering within Phase 2**: relays are tried in descending `(author, R)` score, ties broken by lex-DESC URL (matches the planner selector tiebreak). This gives "best unused option first" without committing to opening all of them at once.

**Concurrency cap per claim**: at most `MAX_EXPANSION_CONCURRENCY = 3`
additional REQs in flight simultaneously, to avoid a connection storm for a
single claim.

**Outcome that advances the phase**:

- **Hit** on any Phase 2 relay: complete the oneshot. Score update: large increment on the delivering `(author, R)`, neutral on still-pending Phase 2 relays whose REQs we then CLOSE.
- **EOSE-without-match** on a Phase 2 relay: neutral score outcome; the slot is
  freed for the next unseen relay (descending score order).
- **Unreachable / failed**: large decrement on `(author, R)`. Slot freed.
- **All outbox relays exhausted with no match**: the oneshot enters the *terminal-exhausted* state. We do NOT keep retrying. The renderer is notified via the existing oneshot completion path (the event will simply not be in the store; the renderer's existing "loading" → "not found" transition is preserved). Score update: nothing new — every relay that contributed an EOSE/fail was already scored.
- **Per-claim wall-clock budget elapsed**
  (`PER_CLAIM_TOTAL_BUDGET_MS = 8000`): terminate the tracked claim as
  `Budget`, distinct from `Exhausted` diagnostics.

### 4.3 Phase 3 — score writeback

**Trigger**: any of EVENT-matched, EOSE-without-match, FailedAfterRetries, claim-terminal frames received by the kernel ingest seam.

**Behaviour**: edge-triggered (D8). The kernel actor — the sole writer (D4) — applies a score delta to the `(author, relay)` cell. The store-layer write is buffered in-memory and flushed on actor idle (LMDB transaction batching; see §8) — no per-frame fsync.

The delta function is owned by `ClaimOutcome` in
`crates/nmp-core/src/kernel/relay_score.rs`: hit increments successes,
EOSE-without-match is neutral, and relay failure increments failures.

---

## 5. Edge Cases

| # | Case | Resolution |
|---|---|---|
| E1 | Relay unreachable mid-claim, after returning some EVENT frames but before EOSE | Treat as Phase advancement event: the EVENT count to date is preserved (those events are persisted to the store and the renderer sees them), but the relay is scored as `Failed` through `Kernel::relay_failed`. |
| E2 | EOSE arrives before the relay's WebSocket is fully open (out-of-order frame from a buffering worker) | Cannot occur: EOSE is keyed on `sub_id`, which doesn't exist until the REQ is sent. The relay_worker invariant from `nmp-network` already prevents this; spec notes it for traceability. |
| E3 | Simultaneous claims for two different events authored by the same author | Registry dedup is keyed on `(scope, shape)`. Different `event_ids` ⇒ different shapes ⇒ no dedup. The two claims run independent Phase 1/2 budgets. Score writes are serialized by the kernel actor (D4), so the second claim's Phase 1 sees scores updated by the first claim's Phase 1 only if those writes have been applied before the second claim's compile pass — by D4 single-writer this is well-ordered. |
| E4 | Simultaneous claims for the *same* event (same `event_ids` filter) | Registry dedups ⇒ one wire REQ, both oneshot tokens complete on the same EOSE/EVENT. Already handled by the existing OneshotApi dedup tests; no new code path needed. |
| E5 | Score decay over time | Scores use exponential age decay with a 14-day half-life. Without decay, a relay that was great a year ago but is now down would stay warm forever, defeating the learning. |
| E6 | Score reset on schema change | The store schema has a version field; bumping it on a schema-incompatible change invalidates all score rows (drop and recreate the table). This is a one-time event (schema bumps are rare and intentional); no graceful migration is required. |
| E7 | Operator removes a relay from `app_relays` after some claims have run | Scores for `(author, that-relay)` persist — the relay can still be picked in Phase 1 via the "already-connected or score ≥ threshold" rule. If the operator wants to forget, scores are *passively* aged out by the decay model. |
| E8 | Author publishes a new NIP-65 list reducing their outbox | Old outbox relays in the score table that are no longer in NIP-65 are *not* automatically purged. They remain candidates if connected; otherwise they are simply never tried (Phase 2 only walks the *current* NIP-65 set). Their stale scores age out via decay. |
| E9 | `app_relays` is empty AND no NIP-65 for the author | Claim immediately terminates as exhausted (see §4.1). Renderer's loading state ends. |
| E10 | Two events authored by the same author land via different `(scope, shape)` paths concurrently — e.g. one via discovery `event_ids`, one via addressable `authors+kinds+d_tags` | Both are independent oneshots; both update scores for the same `(author, relay)` cells. Last-write-wins under D4 single-writer; the actor serializes writes. No lost update. |
| E11 | A relay reports as connected but is silently dropping frames (zombie connection) | Out of scope at the protocol level (relay_worker already detects this via NIP-42 heartbeat / write-failure detection). Phase 2 would still kick in via the wall-clock budget. |
| E12 | Author has a kind:10002 outbox but it lists zero `r` write-relays (empty NIP-65) | Phase 2 has no expansion candidates. Treat the claim as exhausted immediately after Phase 1 EOSE-without-match — do not spin or retry. Scoring impact: Phase 1 EoseNoMatches are neutral (no demerit for a relay that legitimately doesn't carry this event). Acceptance: claim resolves to `terminal-exhausted` within `PHASE_1_BUDGET_MS + epsilon`. |
| E13 | NIP-65 for the author arrives mid-claim — Phase 1 started against `app_relays` only (no outbox known), then the indexer hydrates kind:10002 before Phase 1 elapses | Build the Phase-2 candidate queue lazily at Phase-2 entry, not at claim registration. Reading `MailboxCache.write_relays` is cheap. This means a freshly-arrived outbox is honoured on the same claim's Phase 2 instead of being missed until the next claim. Scoring impact: only Phase-2 outcomes update scores for the newly-discovered relays; the Phase-1 EOSE on `app_relays` is scored against `app_relays`, not the then-unknown outbox. |
| E14 | Relay-URL canonicalisation mismatch — a score row is written under `wss://relay.example.com/` (trailing slash) and read under `wss://relay.example.com` (no slash) | Both `record_claim_outcome` (write) and `is_warm` / `weight` (read) call `CanonicalRelayUrl::parse_or_raw` before keying the score map. Cell consolidation is by canonical form. Scoring impact: a single relay served under multiple textual forms scores as one cell, not many — preserves the "one author, one relay" invariant the score weight assumes. NIP-65 outbox entries (author-provided, not always canonical) and `app_relays` (operator-configured, usually canonical) both flow through the same canonicaliser. |
| E15 | A relay in the Phase 2 candidate list requires NIP-42 AUTH and the kernel has no key bound for it | AUTH-pause is a follow-up outside this contract. The durable scoring rule is that an authentication pause must not be treated as relay unreliability unless the relay actually fails. |
| E16 | Consumer releases an event ref mid-Phase-2 — the user navigated away before the claim completed | Release removes the tracked claim and reverse-index entries. A later claim for the same author starts from the score table state already committed by earlier outcomes. |

---

## 6. Wall-Clock Budgets

The code constants are the source of truth. This table records the product
contract they express.

| Budget | Proposed value | Rationale |
|---|---|---|
| `PHASE_1_BUDGET_MS` | 1500 ms | Most warm-path relays EOSE in <500 ms; 1.5 s comfortably covers slow but live relays. |
| `PER_RELAY_REQ_TIMEOUT_MS` | 5000 ms | A Phase 2 relay that hasn't even ACK'd in 5 s is presumed unreachable. |
| `PER_CLAIM_TOTAL_BUDGET_MS` | 8000 ms | User-visible cap on "still searching"; matches the acceptance-criterion ~5 s target with headroom. |
| `MAX_EXPANSION_CONCURRENCY` | 3 | Avoids opening all 11 of Gigi's outbox relays simultaneously. |
| `MAX_RELAYS_TRIED_PER_CLAIM` | 12 | Hard cap regardless of NIP-65 size. Prevents a pathological list of 50 outbox relays from churning. |

**Doctrine note:** D8 forbids polling. These budgets are enforced by the actor's existing wall-clock-gated observer that drives `drain_lifecycle_tick` — adding new claim-bookkeeping is edge-triggered (frame arrival, observer tick) and never a sleep loop.

---

## 7. Score Data Shape

The accepted scheme is paired counters with exponential age decay.

### Paired counters

```rust
struct Score {
    successes: u32,   // increments on EVENT match
    failures: u32,    // increments on EOSE-no-match / Failed
    last_used_unix_s: u64,  // for decay
}
fn weight(s: &Score, now: u64) -> f32 {
    let age_days = ((now - s.last_used_unix_s) / 86400) as f32;
    let raw = s.successes as f32 / (s.successes + s.failures + 1) as f32;
    raw * (-0.05_f32 * age_days).exp()  // ~14-day half-life
}
```

The warm threshold is 0.40. A clean single hit weighs 0.50 and becomes warm; a
single hit paired with a miss weighs about 0.33 and stays cold. This gives the
claim path a cheap passive-learning memory without a floating EWMA schema or a
hand-rolled Wilson interval.

---

## 8. Data layout and persistence

### 8.1 In-memory

A new field on the kernel:

```rust
// crates/nmp-core/src/kernel/mod.rs
pub struct Kernel {
    // …existing fields…
    relay_author_scores: BTreeMap<(Pubkey, RelayUrl), Score>,
}
```

- `BTreeMap` for deterministic iteration (snapshot stability — relevant for D8 update-equality).
- Keyed on `(Pubkey, RelayUrl)` — generic substrate types, no protocol noun (D0).
- **Single writer: the kernel actor.** No other code path mutates this map (D4).

### 8.2 Persistence

Relay-author scores persist in the LMDB sub-database
`relay-author-scores-v1`. Versioning the sub-database name makes a schema bump a
no-op rename: old score rows remain allocated until the environment is reset,
and the new table starts empty.

- *Write strategy*: actor accumulates deltas in-memory; flushes on idle in the same LMDB transaction as other actor-driven writes (already a thing — see `Kernel::commit_pending_writes` in current code).
- *Read strategy*: load into the in-memory `BTreeMap` on kernel construction. Lazy load (per-author) is rejected for v1 — keeps the read path simple and the working set bounded (an author has at most ~30 outbox relays; total table size is bounded by `|authors_we've_seen| × ~30`).
### 8.3 Snapshot integration

The scores table is NOT included in `AppUpdate` snapshots — it is purely internal kernel state, not a projection the UI consumes. (D8 update-equality is preserved trivially — no Swift/Kotlin code needs the table.)

---

## 9. Doctrine constraints — explicit confirmations

| Doctrine | Constraint | How this design satisfies |
|---|---|---|
| **D0** | No protocol nouns in `nmp-core` | `Score`, `relay_author_scores`, etc. are generic over `(Pubkey, RelayUrl)`. NIP-65 mailbox ownership stays in `nmp-router`; the score table itself is protocol-agnostic. |
| **D4** | `InterestRegistry` is the single writer for sub state | All Phase 1/2 expansion goes through the existing `InterestRegistry::ensure_sub` / `drop_owner`. The expansion adds *more* `LogicalInterest` entries when it advances to Phase 2 — but each entry is registered the same way the original claim is. No bypass. |
| **D6** | No panics, no `Result` across FFI | Score lookups are infallible (`get_or_default`). The Phase 2 trigger emits state, not errors. Operational failure (relay unreachable) surfaces as state fields the renderer already handles ("loading" / "not found"). |
| **D8** | No polling | Every score update is edge-triggered by a frame arrival (EVENT / EOSE / `Kernel::relay_failed`). Phase advancement on wall-clock budget uses the existing actor observer tick, not a `sleep`. Snapshot update equality preserved — the score table is internal-only. |

---

## 10. Implementation Anchors

- `crates/nmp-core/src/kernel/requests/event.rs` registers and releases
  event `resolve_ref` interests.
- `crates/nmp-core/src/kernel/claim_expansion.rs` owns the per-claim phase
  controller.
- `crates/nmp-core/src/kernel/claim_expansion_helpers.rs` builds Phase 2 relay
  candidates and enforces concurrency/fan-out limits.
- `crates/nmp-core/src/kernel/relay_score.rs` owns score constants and cell
  math.
- `crates/nmp-core/src/kernel/relay_score_record.rs` records EVENT, EOSE, and
  relay-failure outcomes.
- `crates/nmp-core/src/kernel/relay_score_flush.rs` flushes dirty score cells.
- `crates/nmp-store/src/lmdb/relay_scores.rs` owns LMDB encoding.
- `crates/nmp-testing/tests/relay_search_radius_*.rs` owns A1-A6 integration
  coverage.

---

## 11. Out of scope (explicit list)

- N1–N4 from §2.
- Profile claim path (different shape; if expansion is wanted there too, it's a follow-up issue).
- Thread hydration (`e`-tag walks).
- Active outbox probing — we do not pre-warm scores in the background.
- iOS / Compose UI state for "still searching" vs "exhausted". The renderer must continue to function with only "event in store" vs "event not in store" as the signal — this design preserves that.
- Cross-relay reputation aggregation ("relay X is generally good"). Strictly per-author scores only.

---

## 12. References

- Issue [#632](https://github.com/pablof7z/nostr-multi-platform/issues/632).
- `crates/nmp-core/src/subs/oneshot.rs` — OneshotApi.
- `crates/nmp-planner/src/selection.rs` — greedy max-coverage selector with `max_per_user`.
- Commit [`680666a0`](https://github.com/pablof7z/nostr-multi-platform/commit/680666a0) — operator-pinned protection (predecessor fix).
- `docs/aim.md` §6 — doctrine list.
