# Relay-Search-Radius Expansion for OneshotApi Event Fetches

**Status**: WIP product spec — no code yet. Anchors PR for issue [#632](https://github.com/pablof7z/nostr-multi-platform/issues/632).

**Scope**: The OneshotApi event-fetch path that backs `nmp_app_claim_event(uri)` (the renderer's "I have a `nostr:` URI, get me the event" entry point). All other OneshotApi shapes (profile claims, thread hydration) are explicitly out-of-scope for this iteration — see §11.

**Doctrines**: D0 (substrate purity in `nmp-core`), D4 (`InterestRegistry` is the single writer), D6 (no panics across FFI), D8 (no polling — every score update is edge-triggered).

This document is the product spec. The implementation plan and the independent review are separate documents (`relay-search-radius-impl-plan.md`, `relay-search-radius-review.md`) produced in Phase 2.

---

## 1. Problem statement

When the renderer triggers `nmp_app_claim_event(uri)` for an embedded event (e.g. a `nostr:naddr1…` article in note content), the planner currently routes the OneshotApi REQ to:

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

- **G1.** Resolve `nmp_app_claim_event` for an event whose author published to ≥1 relay we can reach, even if that relay is not in our app_relays and not in the selector's `max_per_user` picks.
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

Numbered for traceability into Agent A's workstream test plan and Phase 3 integration tests.

**A1.** From a cold gallery TUI launched against `app_relays = [purplepag.es]` only — no Primal, no Gigi-relay — claiming Gigi's article (`naddr1…the-internet-left-me`) resolves within ~5 s, verified by reading `NMP_WIRE_LOG`. The wire log must show:

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

The behaviour is broken into **three phases per claim**, plus a **persistence/learning loop** that bridges across claims. Phase numbers here are claim-internal; do not confuse them with the project's three-phase delivery process (spec / design / implement) from issue #632.

```text
Claim arrives → Phase 1 (warm) → [event found?] → done
                              ↘ [EOSE everywhere / budget elapsed?] → Phase 2 (expansion)
                                                                  → Phase 3 (score writeback)
```

### 4.1 Phase 1 — warm REQ

**Trigger**: A new OneshotApi event-fetch request lands (`nmp_app_claim_event` path; specifically the shape is `InterestShape { event_ids: {…} }` or `InterestShape { authors: {…}, kinds: {…}, d_tags: {…} }` for addressable events). The triggering call site is `Kernel::pending_view_requests` → planner compile cycle.

**Relay set**: union of

- All `app_relays` (operator-pinned; never pruned by the selector — invariant from `680666a0`).
- The author's NIP-65 outbox, **filtered to "preferred"**:
  - Already-connected (no socket-open cost), **OR**
  - Score ≥ `WARM_THRESHOLD` (see §7 — exact value TBD by Agent A).
- If the author has no NIP-65 yet (we haven't seen kind:10002), fall back to `app_relays` only and pre-emptively kick off a NIP-65 fetch for the author. (This already happens via `collect_unknown_refs` indirectly; the spec just notes the dependency.)

**Outcome that advances the phase**:

- **Hit** (EVENT frame matches the claim's filter): the OneshotApi token completes via `complete_unknown_oneshot`. **Score update**: increment `(author, R)` for the relay R that delivered. **Stop.** Other Phase 1 relays may still EOSE; their entries get a *neutral* `seen-but-didn't-deliver` outcome — neither incremented nor decremented (the event might genuinely not be on R; we can't tell whether R is bad or just doesn't have this specific event).
- **EOSE-without-match from every Phase 1 relay**: advance to Phase 2. Each EOSE-without-match decrements the corresponding `(author, R)` by a small amount (less than the increment for a hit) — see §7.
- **Phase 1 wall-clock budget elapsed** (default proposal: 1.5 s — TBD by Agent A): advance to Phase 2 even if some Phase 1 relays are still pending. Pending relays continue running in parallel; their later outcomes still feed the score table.

**Edge cases handled in Phase 1**

- *Phase 1 relay accepts the REQ but never EOSE's.* D8 forbids polling. We use the actor's existing wall-clock-gated observer (the one driving `drain_lifecycle_tick`) to fire a `PhaseTimeout` event when the Phase 1 budget elapses. No `sleep` loop.
- *Phase 1 relay declared unreachable* (worker reports `FailedAfterRetries`): the relay leaves the Phase 1 set; if the set is now empty before the budget elapses, advance to Phase 2 immediately. Score update: large decrement for `(author, R)`.
- *No NIP-65 known, no app_relays configured*: claim immediately fails (cannot search; surfaced via `claim_state = exhausted`). Operator configuration error.

### 4.2 Phase 2 — radius expansion

**Trigger**: Phase 1 advanced as described above.

**Relay set**: the author's NIP-65 outbox **minus** the Phase 1 set. We do not re-query Phase 1 relays in Phase 2 — they've already EOSE'd or are still running.

**Ordering within Phase 2**: relays are tried in descending `(author, R)` score, ties broken by lex-DESC URL (matches the planner selector tiebreak). This gives "best unused option first" without committing to opening all of them at once.

**Concurrency cap per claim**: at most `MAX_EXPANSION_CONCURRENCY` (proposal: 3 — TBD by Agent A) additional REQs in flight simultaneously, to avoid a connection storm for a single claim.

**Outcome that advances the phase**:

- **Hit** on any Phase 2 relay: complete the oneshot. Score update: large increment on the delivering `(author, R)`, neutral on still-pending Phase 2 relays whose REQs we then CLOSE.
- **EOSE-without-match** on a Phase 2 relay: small decrement on `(author, R)`, slot freed for the next unseen relay (descending score order).
- **Unreachable / failed**: large decrement on `(author, R)`. Slot freed.
- **All outbox relays exhausted with no match**: the oneshot enters the *terminal-exhausted* state. We do NOT keep retrying. The renderer is notified via the existing oneshot completion path (the event will simply not be in the store; the renderer's existing "loading" → "not found" transition is preserved). Score update: nothing new — every relay that contributed an EOSE/fail was already scored.
- **Per-claim wall-clock budget elapsed** (proposal: 8 s total from claim arrival — TBD by Agent A): like exhausted, but distinguished in diagnostics. Pending Phase 2 REQs are CLOSE'd. *Open question:* do we let pending Phase 2 REQs run to completion in the background after the user-visible budget expires (so their scores still update)? Recommend yes; see §7.

### 4.3 Phase 3 — score writeback

**Trigger**: any of EVENT-matched, EOSE-without-match, FailedAfterRetries, claim-terminal frames received by the kernel ingest seam.

**Behaviour**: edge-triggered (D8). The kernel actor — the sole writer (D4) — applies a score delta to the `(author, relay)` cell. The store-layer write is buffered in-memory and flushed on actor idle (LMDB transaction batching; see §8) — no per-frame fsync.

The exact delta function is an open question listed in §7 and is for Agent A to resolve.

---

## 5. Edge cases (must be addressed by impl plan)

| # | Case | Resolution |
|---|---|---|
| E1 | Relay unreachable mid-claim, after returning some EVENT frames but before EOSE | Treat as Phase advancement event: the EVENT count to date is preserved (those events are persisted to the store and the renderer sees them), but the relay is scored as if FailedAfterRetries for the current claim. |
| E2 | EOSE arrives before the relay's WebSocket is fully open (out-of-order frame from a buffering worker) | Cannot occur: EOSE is keyed on `sub_id`, which doesn't exist until the REQ is sent. The relay_worker invariant from `nmp-network` already prevents this; spec notes it for traceability. |
| E3 | Simultaneous claims for two different events authored by the same author | Registry dedup is keyed on `(scope, shape)`. Different `event_ids` ⇒ different shapes ⇒ no dedup. The two claims run independent Phase 1/2 budgets. Score writes are serialized by the kernel actor (D4), so the second claim's Phase 1 sees scores updated by the first claim's Phase 1 only if those writes have been applied before the second claim's compile pass — by D4 single-writer this is well-ordered. |
| E4 | Simultaneous claims for the *same* event (same `event_ids` filter) | Registry dedups ⇒ one wire REQ, both oneshot tokens complete on the same EOSE/EVENT. Already handled by the existing OneshotApi dedup tests; no new code path needed. |
| E5 | Score decay over time | *Open question* — Agent A picks decay model from the candidates in §7. Without decay, a relay that was great a year ago but is now down would stay warm forever, defeating the learning. |
| E6 | Score reset on schema change | The store schema has a version field; bumping it on a schema-incompatible change invalidates all score rows (drop and recreate the table). This is a one-time event (schema bumps are rare and intentional); no graceful migration is required. |
| E7 | Operator removes a relay from `app_relays` after some claims have run | Scores for `(author, that-relay)` persist — the relay can still be picked in Phase 1 via the "already-connected or score ≥ threshold" rule. If the operator wants to forget, scores are *passively* aged out by the decay model. |
| E8 | Author publishes a new NIP-65 list reducing their outbox | Old outbox relays in the score table that are no longer in NIP-65 are *not* automatically purged. They remain candidates if connected; otherwise they are simply never tried (Phase 2 only walks the *current* NIP-65 set). Their stale scores age out via decay. |
| E9 | `app_relays` is empty AND no NIP-65 for the author | Claim immediately terminates as exhausted (see §4.1). Renderer's loading state ends. |
| E10 | Two events authored by the same author land via different `(scope, shape)` paths concurrently — e.g. one via discovery `event_ids`, one via addressable `authors+kinds+d_tags` | Both are independent oneshots; both update scores for the same `(author, relay)` cells. Last-write-wins under D4 single-writer; the actor serializes writes. No lost update. |
| E11 | A relay reports as connected but is silently dropping frames (zombie connection) | Out of scope at the protocol level (relay_worker already detects this via NIP-42 heartbeat / write-failure detection). Phase 2 would still kick in via the wall-clock budget. |

---

## 6. Wall-clock budgets

All budget numbers below are **proposals** for Agent A to confirm or refine based on the m16 wire-log timing data.

| Budget | Proposed value | Rationale |
|---|---|---|
| `PHASE_1_BUDGET_MS` | 1500 ms | Most warm-path relays EOSE in <500 ms; 1.5 s comfortably covers slow but live relays. |
| `PER_RELAY_REQ_TIMEOUT_MS` | 5000 ms | A Phase 2 relay that hasn't even ACK'd in 5 s is presumed unreachable. |
| `PER_CLAIM_TOTAL_BUDGET_MS` | 8000 ms | User-visible cap on "still searching"; matches the acceptance-criterion ~5 s target with headroom. |
| `MAX_EXPANSION_CONCURRENCY` | 3 | Avoids opening all 11 of Gigi's outbox relays simultaneously. |
| `MAX_RELAYS_TRIED_PER_CLAIM` | 12 | Hard cap regardless of NIP-65 size. Prevents a pathological list of 50 outbox relays from churning. |

**Doctrine note:** D8 forbids polling. These budgets are enforced by the actor's existing wall-clock-gated observer that drives `drain_lifecycle_tick` — adding new claim-bookkeeping is edge-triggered (frame arrival, observer tick) and never a sleep loop.

---

## 7. Score data shape — open question

The issue explicitly leaves the scoring schema open. Agent A must pick from these candidates (or propose a better one) with file:line-level justification.

### Candidate A — paired counters

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

- **Pros**: trivial to reason about; restart-stable; easy to debug ("Gigi/dergigi: 17 successes, 3 failures").
- **Cons**: cold-start cells (1 hit, 0 misses) look identical to 100% confidence; no early statistical discounting.

### Candidate B — EWMA float

```rust
struct Score { ewma: f32, last_used_unix_s: u64 }
// On hit: ewma = ALPHA * 1.0 + (1 - ALPHA) * ewma
// On miss: ewma = ALPHA * 0.0 + (1 - ALPHA) * ewma
// With ALPHA = 0.2: ~5-event memory; warmer recent outcomes weighted more.
```

- **Pros**: built-in decay-by-recency; smooth.
- **Cons**: not restart-stable across schema changes (f32 representation needs careful serialization); harder to debug; loses cardinality info ("how many trials").

### Candidate C — Wilson lower-bound

```rust
fn wilson_lower(successes: u32, n: u32) -> f32 {
    // Wilson score interval, lower bound at z=1.96 (95% CI)
    // …
}
```

- **Pros**: principled cold-start (1/1 ≠ 17/17); the standard for "ranking by upvotes" problems.
- **Cons**: heaviest math; agent must hand-implement; harder to explain to operators inspecting the table.

**Recommendation for Agent A**: prefer Candidate A for v1. It's restart-trivial, debuggable, and the decay multiplier covers recency. The cold-start ambiguity (1/1 vs 100/100) doesn't actually matter for Phase 1 selection: anything above `WARM_THRESHOLD` is preferred; ranking among warm cells uses lex-DESC URL as the secondary tiebreak. Revisit if telemetry shows we're warming relays that have only one trial.

**Open question for Agent A**: name the exact `WARM_THRESHOLD` value under the chosen scheme.

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

**Recommendation**: a dedicated LMDB table in `nmp-nostr-lmdb`, name `relay_author_scores_v1`. Versioning the table name (vs. having a schema-version row) makes a schema bump a no-op rename — the old table is silently abandoned, scores reset, and the new table is empty. This matches §5 E6.

- *Write strategy*: actor accumulates deltas in-memory; flushes on idle in the same LMDB transaction as other actor-driven writes (already a thing — see `Kernel::commit_pending_writes` in current code).
- *Read strategy*: load into the in-memory `BTreeMap` on kernel construction. Lazy load (per-author) is rejected for v1 — keeps the read path simple and the working set bounded (an author has at most ~30 outbox relays; total table size is bounded by `|authors_we've_seen| × ~30`).
- *Alternative* (call out for Agent A): a flat file in the kernel's data dir, JSON-serialized on idle. Simpler than LMDB but loses transactional consistency with other store writes. Likely rejected; document the trade.

### 8.3 Snapshot integration

The scores table is NOT included in `AppUpdate` snapshots — it is purely internal kernel state, not a projection the UI consumes. (D8 update-equality is preserved trivially — no Swift/Kotlin code needs the table.)

---

## 9. Doctrine constraints — explicit confirmations

| Doctrine | Constraint | How this design satisfies |
|---|---|---|
| **D0** | No protocol nouns in `nmp-core` | `Score`, `relay_author_scores`, etc. are generic over `(Pubkey, RelayUrl)`. No NIP-XX naming. The fact that we *use* NIP-65 to derive the outbox is handled in an existing module (`nmp-nip65` adapter); the score table itself is protocol-agnostic. |
| **D4** | `InterestRegistry` is the single writer for sub state | All Phase 1/2 expansion goes through the existing `InterestRegistry::ensure_sub` / `drop_owner`. The expansion adds *more* `LogicalInterest` entries when it advances to Phase 2 — but each entry is registered the same way the original claim is. No bypass. |
| **D6** | No panics, no `Result` across FFI | Score lookups are infallible (`get_or_default`). The Phase 2 trigger emits state, not errors. Operational failure (relay unreachable) surfaces as state fields the renderer already handles ("loading" / "not found"). |
| **D8** | No polling | Every score update is edge-triggered by a frame arrival (EVENT / EOSE / FailedAfterRetries). Phase advancement on wall-clock budget uses the existing actor observer tick, not a `sleep`. Snapshot update equality preserved — the score table is internal-only. |
| **Article VII (Simplicity Gate)** | No future-proofing | We are not building a "relay reputation service" — only the minimum to pass A1–A6. |

---

## 10. Open questions (for Agent A to resolve in the impl plan)

1. **Score scheme** — Candidate A vs B vs C from §7. Recommendation: A.
2. **`WARM_THRESHOLD`** — exact numeric threshold under the chosen scheme.
3. **Wall-clock budgets** — confirm or refine the proposals in §6 against the m16 wire-log timing data.
4. **Decay half-life** — proposal in §7 is 14 days; confirm.
5. **Persistence target** — LMDB table vs. JSON-on-idle. Recommendation: LMDB.
6. **Phase 2 ordering tiebreaker** — lex-DESC URL is the strawman; Agent A confirms or proposes an alternative (e.g. NIP-11 supported_nips intersection with the claim's kind).
7. **Background-completion of Phase 2 REQs after user-visible budget** — recommend yes (still update scores); confirm.
8. **Where does the "claim came in" entry point hand off to the new expansion controller?** A new module `crates/nmp-core/src/kernel/claim_expansion.rs`? Or extend `discovery.rs`? Agent A picks based on file-size budget (D-V12: 500 LOC ceiling).
9. **Wire-level: do we open one REQ per relay in Phase 2 or batch?** The OneshotApi `(scope, shape)` is the same — one logical interest. The planner already partitions per-relay (`per_relay` map in `CompiledPlan`). Expansion likely re-runs `apply_selection` with a different `max_per_user` for the claim's specific sub-shape. Agent A defines the exact planner extension point.
10. **Telemetry / wire-log** — what `NMP_WIRE_LOG` lines do we emit for Phase advancement? Acceptance test A1 depends on these being legible.

---

## 11. Out of scope (explicit list)

- N1–N4 from §2.
- Profile claim path (different shape; if expansion is wanted there too, it's a follow-up issue).
- Thread hydration (`e`-tag walks).
- Active outbox probing — we do not pre-warm scores in the background.
- iOS / Compose UI state for "still searching" vs "exhausted". The renderer must continue to function with only "event in store" vs "event not in store" as the signal — this design preserves that.
- Cross-relay reputation aggregation ("relay X is generally good"). Strictly per-author scores only.

---

## 12. Process

Per issue #632, this PR follows the three-phase workflow:

1. ✅ **Phase 1 — spec PR (this document)**. WIP PR open. Issue linked.
2. **Phase 2 — design and review**. Agent A writes `relay-search-radius-impl-plan.md` (workstream breakdown W1..Wn, file:line specifics, doctrine guards, test plans). Agent B reviews both this spec and Agent A's plan; writes `relay-search-radius-review.md` with file:line concerns.
3. **Phase 3 — implementation**. Workstreams land into this same PR; A1–A6 verified.

A separate concern surfaced during Phase 1: the agent harness in this session does not expose a sub-agent dispatch tool. Phase 2 will either be executed sequentially in-session (with planner and reviewer roles clearly labeled) or via a sub-process to the `claude` CLI. Either way, both documents land as commits on this PR before Phase 3 starts.

---

## 13. References

- Issue [#632](https://github.com/pablof7z/nostr-multi-platform/issues/632).
- `crates/nmp-core/src/subs/oneshot.rs` — OneshotApi.
- `crates/nmp-core/src/kernel/discovery.rs` — claim/discovery seam, the analogue to imitate.
- `crates/nmp-planner/src/selection.rs` — greedy max-coverage selector with `max_per_user`.
- Commit [`680666a0`](https://github.com/pablof7z/nostr-multi-platform/commit/680666a0) — operator-pinned protection (predecessor fix).
- `docs/aim.md` §6 — doctrine list.
- `docs/plan/m16-kind-dispatch-handlers.md` — workstream-style plan to model Agent A's output after.
