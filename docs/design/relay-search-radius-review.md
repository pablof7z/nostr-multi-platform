# Relay-Search-Radius Review

> Independent opus reviewer feedback on
> [`relay-search-radius.md`](relay-search-radius.md) (product spec) and
> [`relay-search-radius-impl-plan.md`](relay-search-radius-impl-plan.md)
> (workstream plan). Phase 2 of issue
> [#632](https://github.com/pablof7z/nostr-multi-platform/issues/632).
>
> Convention: every claim is grounded with a `file:line` against
> `feat/relay-search-radius-expansion` at `f6f043ac`. Where I "checked
> and found nothing", I say so explicitly with one-sentence justification
> rather than padding.

---

## A. Architectural critique

### A.1 Score scheme — paired counters (sound, justification thin)

Plan §0 Q1 picks Candidate A (paired counters + exponential decay) over EWMA
(B) and Wilson lower-bound (C). The choice itself is fine — restart-stability
under a fixed LMDB record (24 bytes) is a stronger constraint here than
statistical purity, and the cold-start ambiguity (1/1 ≈ 0.50 vs 100/0 ≈ 1.00)
is well-handled by `WARM_THRESHOLD` being a single floor rather than a ranking
gate. The justification at impl-plan.md:20 is fine.

What is **thin** in the justification: the spec at §7 says "the cold-start
ambiguity … doesn't actually matter for Phase 1 selection: anything above
`WARM_THRESHOLD` is preferred". That's only true if Phase 1 admits *every*
warm relay irrespective of count — which contradicts impl-plan.md:21 where
`WARM_THRESHOLD = 0.40` deliberately excludes one-hit-one-miss cells. The
plan handles this fine, but the **spec text needs a one-line patch** so the
two documents agree on what "cold-start ambiguity doesn't matter" actually
means.

**Sharpen** by citing the test in W1 case 2 (impl-plan.md:107) — a
`successes=1, failures=1` cell evaluates to `1/3 ≈ 0.33`, excluded by the
0.40 floor; that's the spec's recovery path from the cold-start ambiguity
and should be quoted in spec §7.

### A.2 Persistence target — LMDB sub-db (sound, with two follow-ups)

Plan §0 Q5 picks the LMDB sub-db `relay_author_scores_v1` over JSON-on-idle.
This is **correct on transactional grounds** (`additional_dbs` is already a
parameter at `crates/nmp-nostr-lmdb/src/store/lmdb/mod.rs:131-146`,
`open_env` signature still takes `additional_dbs: u32`). Two unaddressed
follow-ups:

1. **Orphan sub-db on schema bump.** §0 Q5 says "schema-bump = name bump;
   no migration code" (§5 E6 echoes this). LMDB does **not** reclaim
   storage for an abandoned sub-db — it stays on disk until the operator
   drops the env. This is acceptable (schema bumps are rare) but the plan
   should state the trade explicitly. **Recommended amendment**: spec §5
   E6 adds "the old sub-db's pages remain allocated until env-reset; this
   is intentional and v1-acceptable".
2. **Hash-of-pubkey key-prefix vs lex-sorted (pubkey,url) key.** Plan W2
   line at impl-plan.md:148 says the key is "`[32-byte pubkey hex
   bytes][url bytes]` — variable-length but pubkey-prefix sortable". That
   is *not* what the encoding describes — the hex form is 64 bytes, not
   32, and "url bytes" is variable. Either correct to "64-byte hex" or
   switch to the 32-byte raw form. Minor but the implementer will hit it
   on the first roundtrip test.

### A.3 Wall-clock budgets — sound values, broken justification

Plan §0 Q3 confirms `PHASE_1_BUDGET_MS=1500`, `PER_RELAY_REQ_TIMEOUT_MS=5000`,
`PER_CLAIM_TOTAL_BUDGET_MS=8000`. The values are reasonable for the
acceptance criterion ("~5 s, headroom to 8 s"). The **justification is
factually wrong** in one place:

- impl-plan.md:22 says: *"8 s sits inside one reconnect backoff
  (`RELAY_RECONNECT_DELAY_INITIAL`)"*.
- Actual: `RELAY_RECONNECT_DELAY_INITIAL: Duration = Duration::from_secs(3)`
  at `crates/nmp-network/src/relay_protocol.rs:32`. 8 s is **2.6×** the
  initial backoff. The keepalive claim (1.5 s inside 30 s `keepalive_idle`)
  is correct.

This is not a knob-direction error; the budgets are still fine. But the
plan grounds a numerical decision in a fact it didn't verify, and that's
the kind of error compounds when the implementer trims a budget against
the same fact later. **Fix**: rewrite the justification against the actual
constants (`KEEPALIVE_IDLE`, the m16 wire-log p95 latency if/when measured,
and the per-relay RTT median observed in real-relay smoke tests).

The spec **§6 also overpromises measurement**: "all budget numbers below
are proposals for Agent A to confirm or refine based on the m16 wire-log
timing data". There is no such timing data in-tree (`docs/plan/m16-*.md`
do not contain p95/p99 EOSE-latency tables). The plan punts to "revisit
when wire-log telemetry from W8 lands" (impl-plan.md:22) which is the
right honest answer; spec §6 should adopt the same framing instead of
implying the data exists.

### A.4 Phase 2 trigger — race against EOSE-without-event

Spec §4.1 advances Phase 2 on either (a) every Phase-1 relay EOSEs without
a match, or (b) `PHASE_1_BUDGET_MS` elapses. The plan implements both via
`Kernel::poll_claim_expansion` (W5, called from the actor idle tick) plus
the W3 score-update hooks.

**Race I see and the plan does not call out**:

The Phase 1 set is the union of `app_relays ∪ warm-outbox`. An EVENT
match arriving from any Phase 1 relay at the same idle tick as the
budget elapses produces two parallel state transitions: (i) the W3
`Hit` hook completes the oneshot and increments the success counter;
(ii) `poll_claim_expansion` reads the still-pre-completion claim entry
and tries to advance to Phase 2 anyway, opening N new wire REQs.

The plan partly addresses this: W5 says `Kernel::on_claim_outcome` is
called from W3 hooks and "advances the per-claim state on Hit (terminate
claim, drop pending entry)". But the actor's idle-tick order at
`crates/nmp-core/src/actor/mod.rs:1554` is `drain_lifecycle_tick`
*before* the proposed `poll_claim_expansion`. EVENT ingest does not
flow through `drain_lifecycle_tick`; it's a separate `handle_text`
branch (`kernel/ingest/mod.rs:160-165`) which runs synchronously on the
inbound-message dispatch. If the EVENT arrives during the same idle
section, ordering depends on which branch `tick.rs` selects first —
not enforced by the plan.

**Recommendation**: the plan must specify *the synchronization
contract* — `Kernel::poll_claim_expansion` must check
`event_already_known(&primary_id)` (the existing helper at
`crates/nmp-core/src/kernel/requests/event.rs:282`) before emitting
Phase-2 REQs, OR `on_claim_outcome::Hit` must set a per-claim
"terminal" flag that `poll_claim_expansion` always checks first. This
is a one-line invariant that's load-bearing for not double-fetching;
silently relying on tick ordering is brittle.

### A.5 D4 single-writer on the score table

The score table mutates only via `Kernel::record_claim_outcome` (W3),
which is `&mut self` on the actor-owned Kernel. The trait-shape in W2
gives `RelayAuthorScoreStore::put_batch(&mut self, ...)`; reads from
the planner go via `Arc<RelayAuthorScoreLookup>` (W4) which reads the
*in-memory* map (impl-plan.md:300).

**No path I found** where a non-actor mutates the map. The trait
exposes a read-only `weight` / `is_warm`. The `Arc<dyn
RelayAuthorScoreLookup>` proposal in W4 wraps a read-only view, not a
mutable handle — that's correct.

**One ambiguity**: impl-plan.md:299 says
`KernelMailboxes` gains `Arc<RelayAuthorScoreLookup>`. The current
`KernelMailboxes` (built in `crates/nmp-core/src/kernel/mailboxes.rs`)
is constructed *per call* (`drain_lifecycle_tick` at
`kernel/lifecycle_drain.rs:37` rebuilds it on every tick). Snapshotting
the score map into the `Arc` means the planner reads a frozen view from
the start of the tick — **fine for D4 correctness** (single-writer
trivially) but observation-stale by one tick relative to W3 deltas.
Acceptance criterion A6 wants claim B's Phase 1 to see claim A's
delta; if both register in the same tick, the planner will not see A's
update until *the following* tick. The plan asserts this works
(impl-plan.md:721 R1) but the explicit "one tick of delay is
acceptable / fails / is corrected by …" should be in the plan.

**Recommendation**: rather than wrapping the score map in `Arc`,
expose a `&self` `RelayAuthorScoreLookup for Kernel` impl that the
planner consults directly. The advisor flagged this — the Arc is
cargo-cult; the score map only mutates on the actor thread and only
reads from the planner on the same actor thread inside
`drain_lifecycle_tick`. A `&self` lookup serializes naturally with W3
mutations; the Arc-wrapped snapshot serialises *less* naturally
because A6's same-tick-visibility expectation is then broken by
construction.

### A.6 File-size deltas vs D-V12

Hard ceilings I verified:

| File | Current LOC | Plan delta | Post |
|---|---|---|---|
| `crates/nmp-core/src/kernel/mod.rs` | **1952** | +6 (W1) +6 (W5) | 1964 (worse) |
| `crates/nmp-core/src/kernel/ingest/mod.rs` | **645** | +35 (W3) | 680 |
| `crates/nmp-core/src/kernel/discovery.rs` | 251 | 0 | 251 |
| `crates/nmp-core/src/kernel/requests/event.rs` | 310 | +5 (W5 hook) | 315 |
| `crates/nmp-planner/src/selection.rs` | 340 | +60 (W4) | 400 |
| `crates/nmp-planner/src/compiler/partition/case_a_authors.rs` | 253 | +40 (W7) | 293 |
| `crates/nmp-planner/src/compiler/partition/case_b_addresses.rs` | 176 | +40 (W7) | 216 |

Plan §7.6 already acknowledges `ingest/mod.rs` is over the 500 LOC ceiling
*pre-feature* (645) and elevates the `kernel/ingest/eose.rs` extraction
from "if it overflows" to "required pre-refactor". **Concur** — that
should be commit 1 of W3 and gated by the smoke test. The same logic
should apply to `selection.rs`: 340 → 400 is under the ceiling, but
plan W4 reads the file as ~340 LOC and budgets +60; that math is fine
*only* because the file is already under 400. Mention this explicitly
so the implementer doesn't budget themselves into a violation by
adding more than 60.

`kernel/mod.rs` is **already 1952 LOC**, far over the ceiling
(impl-plan.md:911-916 acknowledges this — current `wc -l` matches).
The +12 LOC across W1 and W5 is acceptable (V-12 backlog), but the
strict "no method bodies in mod.rs" rule (impl-plan.md:722, R2 mitigation)
is what keeps this from getting worse and must be enforced.

---

## B. Doctrine violation hunt

### B.1 D0 (substrate purity in `nmp-core`)

I checked: `RelayAuthorScore` (W1), `RelayAuthorScoreStore` (W2),
`RelayAuthorScoreLookup` (W4), `PendingClaim` (W5) all key on
`(Pubkey, RelayUrl)`. No protocol noun in the type names. The trait
home in `crates/nmp-core/src/substrate/` is correct precedent (matches
`MailboxCache` at `crates/nmp-core/src/substrate/routing.rs:276`). The
LMDB impl-side lives in `nmp-nostr-lmdb` (impl-plan.md:144). **No
D0 risk** I can substantiate.

The spec §9 cites "Article VII (Simplicity Gate)". I searched
`docs/aim.md`, `AGENTS.md`, and `docs/` broadly — **the citation does
not resolve**. `aim.md` §6 numbers 12 doctrines but uses no Roman
numerals or "Article" framing. The closest invariant is doctrine 8
("No business logic in native code" + "Bounded native state"), not a
simplicity gate. **Recommend**: spec §9 either cite the actual
source ("aim.md §6 doctrine N" or "AGENTS.md §X") or drop the
citation; "Simplicity Gate" as an unattributed principle should not
appear in a planning doc.

### B.2 D4 (`InterestRegistry` is the single writer for sub state)

W5's claim-expansion controller calls `oneshot.request` (per
impl-plan.md:28 Q9: "one new `LogicalInterest` per Phase 2
candidate") which goes through `OneshotApi.request` →
`registry.ensure_sub` (`crates/nmp-core/src/subs/oneshot.rs:127`).
That is the same path `discovery.rs:152` already uses. **No D4 risk**
on the sub side.

**Risk on the score side**: the plan does NOT bypass D4 — the score
map is not a sub-state table, so D4 doesn't technically apply to it,
but the analog-doctrine "single writer for actor-owned state" is
satisfied because W3's `record_claim_outcome` is the only mutator
and is `&mut self`. The `Arc`-wrapping issue (A.5 above) is a
*read-side* concern, not D4.

### B.3 D6 (no panics, no `Result` across FFI)

W2 trait methods return owned `Vec` / take `&mut self` (impl-plan.md:152).
W3's `record_claim_outcome` is total (impl-plan.md:252). W5's
`Kernel::register_claim_expansion`, `poll_claim_expansion`,
`on_claim_outcome` all return `Vec<OutboundMessage>` or `()` — no
`Result` (impl-plan.md:361-377). W8's `log_wire` is
`unwrap_or_default()` over JSON encode (impl-plan.md:594) which means
an unrenderable event silently produces `""` — never a panic.

**One risk**: W7 hint-walk in `case_a_authors.rs` and
`case_b_addresses.rs` operates on `canonical_relay_url`
(impl-plan.md:545: "malformed hints (non-canonical URL) are dropped
silently"). The current code at
`crates/nmp-planner/src/compiler/partition/case_a_authors.rs` does
NOT today route any hint, so this is a new lane. Confirm during
implementation that `canonical_relay_url::canonicalize` is `total`
(returns `Option`/`Result` but never panics on user-typed garbage).
The advisor flagged W3 placement separately; W7's D6 risk is
acceptable as long as the implementer keeps the `if let Some(url)`
guard.

### B.4 D8 (no polling)

The plan attempts to claim D8 by piggy-backing on the existing actor
idle-tick (`crates/nmp-core/src/actor/mod.rs:1554`). Both
`poll_claim_expansion` (W5) and `flush_relay_scores_if_dirty` (W2)
add new callees there. **This is the right pattern** — same shape as
`pending_sign.retain_mut` (`crates/nmp-core/src/actor/pending_sign.rs`)
and `tick_publish_engine_for_now`.

**Riskiest spot**: W8's `env::var_os("NMP_WIRE_LOG").is_none()` early
return (impl-plan.md:592) is called from W3's `record_claim_outcome`
(impl-plan.md:600 — "~10 sites"). That's once per EVENT and once per
EOSE on hot inbound. An OS syscall per call is **measurable** on the
ingest hot path. Plan §7.4 retargets the env var to `NMP_CLAIM_LOG`
but does not change this. **Required**: cache the boolean in a
`OnceLock<bool>` at module init (R5 says "measure during W9 to
confirm"; I'd flip the default — always cache, opt out only if
measurement shows the cache hurts). The cost of `OnceLock<bool>::get`
is one atomic load; this is the right default.

Also: the spec at §4.1 calls Phase 1 timeout enforcement
"edge-triggered (D8)". It's actually **wall-clock-gated on the actor
idle tick** — not edge-triggered. The phrasing in spec §4.1 needs to
match the plan's W6 (impl-plan.md:484): same wall-clock observer the
existing `pending_sign.timed_out()` uses, no `sleep` loop. The
distinction matters because "edge-triggered" implies an external
event drives the timeout (e.g. an incoming frame), which isn't the
case here.

### B.5 Article VII / Simplicity Gate

I checked `docs/aim.md`, `AGENTS.md`, and `docs/` broadly. **The
citation does not resolve to any in-tree source.** Treat as
unattributed and either drop or replace with the actual doctrine
("Pit-of-success" from
`docs/product-spec/api-design-philosophy.md` is the closest in spirit
but covers a different angle). No risk-of-violation to evaluate
because the principle itself is not anchored.

---

## C. Missing edge cases

Spec §5 lists E1–E11. Significant cases the spec omits:

**E12 — Author has empty NIP-65 outbox** (kind:10002 with zero `r`
tags, write-relays-only-set is empty). Spec §4.1 says "if no NIP-65
yet … pre-emptively kick off a NIP-65 fetch". But what if NIP-65
*exists* and is empty? Phase 2 has nothing to expand into. Should
this fall through to `app_relays`-only Phase 1 retry, or terminate
exhausted? Plan does not say.

**E13 — NIP-65 arrives mid-claim.** A claim starts with no NIP-65
for the author (Phase 1 = `app_relays` only). Mid-Phase-1 the
indexer hydrates kind:10002 for that author. Does Phase 2 expansion
now use the freshly-arrived outbox, or is the candidate set frozen
at claim-arrival? The plan's `PendingClaim.candidate_queue`
(impl-plan.md:387) is built once; the spec doesn't say.
**Recommendation**: rebuild candidate queue lazily when Phase 2
begins, not at claim-arrival; cheap (one `MailboxCache.write_relays`
call) and correct.

**E14 — Relay-URL canonicalization mismatch.** The ingest path
canonicalises URLs (`CanonicalRelayUrl::parse_or_raw` at
`kernel/ingest/mod.rs:144`); the planner's `apply_selection` keys on
`RelayUrl` strings; NIP-65 outbox entries are author-provided and
might not be canonical. A score row written under
`wss://relay.example.com/` and read under `wss://relay.example.com`
(trailing slash) is a different cell. Plan W2's key encoding
(impl-plan.md:148) does not call out canonicalization. **Required**:
canonicalize before scoring AND before lookup; mention in W1/W2.

**E15 — AUTH-required Phase 2 relay.** A relay in Phase 2's
candidate list requires NIP-42 AUTH. Today's auth gate
(`crates/nmp-core/src/kernel/auth.rs` + `auth_gate.rs`) parks the
REQ until authenticated. Plan does not specify whether the
per-relay AUTH-pause counts against `PER_RELAY_REQ_TIMEOUT_MS`. If
it does, AUTH-required relays in Phase 2 effectively never
contribute to scoring (parked for 5 s, then implicit failure). If
it doesn't, the per-claim budget can blow through. **Recommended
spec amendment**: AUTH-pause excludes the relay from Phase 2 and
records a neutral score outcome — never a `Failed` decrement just
because the relay wanted AUTH.

**E16 — `release_event` arrives mid-Phase-2.** A consumer releases
its claim before Phase 2 resolves. The spec doesn't say what
happens to in-flight Phase 2 REQs. Plan §0 Q7 says "background-
complete Phase 2 after user budget = Yes". By the same logic,
should release also let Phase 2 background-complete, or CLOSE the
in-flight REQs? `release_event` already removes the claim from
`event_claims` and `event_claim_requested`
(`requests/event.rs:256-262`) but does NOT release the OneshotApi
token (the EOSE handler does that). If Phase 2 has multiple
oneshots in flight, only one is the dedup target of EOSE handling.
**Recommended**: Phase 2 in-flight REQs continue (matches §0 Q7
rationale: score-learning is worth the cost). Confirm during W5.

I checked for more but the remaining gaps are sub-cases of E1/E11
or duplicates. **Three is the floor; five if you count E15 and
E16 as substantive.**

---

## D. Test plan gaps

Walking W1..W9:

### W1 (`relay_score_tests.rs`)

Plan tests 1–6 cover weight monotonicity and decay arithmetic. **Missing**:
- Saturating-add overflow on `successes: u32` after 2^32 hits — defensive,
  but `u32::MAX + 1` on a single-author/single-relay cell shouldn't panic
  (D6). The plan never tests `saturating_add`.

### W2 (LMDB persistence)

Tests 1–4 cover roundtrip, schema reset, dirty-flag, and full kernel
restart (A3 underwrite). **Missing**:
- **Concurrent LMDB env open from a sibling reader** — the existing
  smoke (`real_relay_outbox.rs`) opens one env; the kernel under test
  may share an env with a Negentropy reader or `claimed_events` reader.
  Confirm `additional_dbs` reservation does not break the sibling open.
- **Encoding-roundtrip with non-canonical URL** — E14 above. Tests
  should write under `wss://r.example/` and read under
  `wss://r.example` and assert they hit the **same** cell (after
  canonicalization). Without this test, the rounding-trip cell-loss
  ships silently.

### W3 (score-update seam)

Tests 1–5 cover hit / EOSE-no-match / failed delta arithmetic plus a
wire-shaped test using the discovery_tests precedent. **Missing**:
- **`FailedAfterRetries` vs `relay_failed` confusion** — plan
  impl-plan.md:230 says to hook the failure path on
  `kernel/relay_transport.rs::FailedAfterRetries`. The advisor
  confirmed and I verified: `FailedAfterRetries` is publish-engine
  terminology (see `kernel/publish_engine_terminals.rs:100`,
  `kernel/publish_engine_tests.rs:202`), **NOT** a transport-side
  callback. The transport-failure entry-point is
  `Kernel::relay_failed` at
  `crates/nmp-core/src/kernel/requests/relay_lifecycle.rs:73`.
  Plan W3 needs to retarget to `relay_failed` and a per-claim
  filter ("decrement only for authors whose Phase-1/2 set included
  the failed relay"); a test for that filter is mandatory.
- **Score decrement on relay-side CLOSED** (NIP-01 CLOSED frame
  with reason). The ingest path at `kernel/ingest/mod.rs:253-303`
  handles CLOSED; scoring it as a failure is questionable
  (auth-required CLOSED is not a relay quality signal). The plan
  is silent; either spec it or test the explicit decision.
- **Hit-after-EoseNoMatch**: relay R EOSEs without match, then
  delivers an EVENT on a *later* claim. Does the previous
  decrement still apply, or does the hit erase it? Plan's delta
  arithmetic answers "the hit increments successes, leaves
  failures alone" — but no test asserts this idempotency.

### W4 (planner warm-relay preference)

Tests 1–4 cover warm filter, operator-pinned bypass, noop lookup
regression, and adapter wiring. **Missing**:
- **Integration with `apply_selection`'s wildcard-author
  preservation** (`crates/nmp-planner/src/selection.rs:208-211`).
  A relay whose only sub-shape is a wildcard
  (e.g. gift-wrap `#p` filter for the same author) must not be
  affected by the score filter. The plan never tests this; the
  current selector preserves wildcard sub-shapes (line 209), and
  the new warm-filter must not regress it.
- **Greedy interaction**: when the warm filter prunes a
  per-relay author set to a singleton, does the greedy max-cover
  still pick that relay if it adds nothing else? Test the
  degenerate case `score_lookup` filtering everything except one
  niche relay → assert the niche relay survives.

### W5 (claim_expansion controller)

Tests 1–8 cover the bulk of phase transitions including
concurrency cap, ordering, exhaustion, and the A6 same-author
race. **Missing**:
- **Phase 1 hit AT THE SAME tick as Phase 1 timeout** — the
  race in A.4 above. Without this, the implementer relies on tick
  ordering implicitly.
- **Phase 2 expansion-against-empty-outbox** (E12). Author has
  kind:10002 with zero `r` tags. Today's `MailboxCache.write_relays`
  returns `Some(Vec::new())` (verify in `nmp-nip65`). Phase 2
  must terminate gracefully, not spin.
- **`PER_RELAY_REQ_TIMEOUT_MS` enforcement** — there's no test for
  the per-relay timeout (5 s). The only timeout exercised is
  `PER_CLAIM_TOTAL_BUDGET_MS`. The per-relay timeout is the only
  thing protecting against a single slow Phase 2 relay starving
  the candidate queue.

### W6 (deadline tick)

Plan extends `t142_drain_tick_actor_idle_loop` and notes the 4 Hz
emit cadence is "comfortably inside the budget's resolution
requirement". **Missing**: an integration test that the **actual
elapsed** time between `register_claim_expansion` and Phase 2 REQ
emission is in `[PHASE_1_BUDGET_MS, PHASE_1_BUDGET_MS +
250ms_emit_period]`. Without this, regressing `DEFAULT_EMIT_HZ`
silently breaks the budget contract.

### W7 (hint consumption)

Tests 1–5 cover hint routing in case_a and case_b. **Missing**:
- **Case_d_no_author** is the case that today sets `hints:
  Vec::new()` (verified at
  `crates/nmp-planner/src/compiler/partition/case_d_no_author.rs:158,257`).
  Plan W7 scope at impl-plan.md:524 says case_a and case_b only —
  but case_d may also see Phase 2 hints. Test that case_d either
  honours or explicitly skips hints; silent drop is wrong.
- **Hint canonicalization mismatch with `app_relays`**: a hint
  URL spelled differently from the same operator-pinned app relay
  should dedupe to one `RelayEntry`, with both `Hint` and
  `UserConfigured(AppRelay)` in `role_tags`. Plan §0 Q9 dedup
  test (impl-plan.md:556 case 3) covers `Nip65`+`Hint` but not
  `AppRelay`+`Hint`.

### W8 (NMP_CLAIM_LOG)

Tests 1–3 cover env-gate. **Missing**: the **A1 assertion shape**.
The acceptance test at impl-plan.md:733 grep-checks `ReqEmit
phase=phase1`; if the emitter formats `phase=Phase1` (case-mismatch
between `Phase` enum's `Debug` and the line text) the assertion
silently fails or false-passes. Test the exact emitted-line shape
once, in W8's unit suite, against a fixture; A1 then asserts
*against the same fixture format* — not against a hand-typed grep.

### W9 (acceptance integration)

Plan ships A1–A6 as gated `--features real-relay` tests, each in
its own file under `crates/nmp-testing/tests/`. **Missing**:
- **A5's stub-relay shape** (impl-plan.md:656) is "spawn a stub
  relay that drops the connection mid-claim". The existing
  test_relay precedent in `crates/nmp-testing` is for plain
  WebSocket relays, not stub-with-disconnect — needs explicit
  test infrastructure beyond what the plan accounts for
  (probably +100 LOC in `common/`). Budget impact unaccounted
  for.
- **Real-relay flakiness budget**. The plan says A1 resolves in
  "< 5500 ms wall-clock" (impl-plan.md:733). Real relays have
  long-tail latency (p99 > 5 s seen on `nos.lol`, `nostr.wine`).
  Either the test reads p50/p95 from a m16 baseline or it
  flake-budgets to `< 8000 ms`; bias toward the latter (matches
  `PER_CLAIM_TOTAL_BUDGET_MS`).
- **A2 score-priming idempotency**: A2 says "After A1 priming +
  2nd claim". A1's wire-log assertion includes a Phase 2 `ReqEmit`
  on Gigi's outbox; that REQ is recorded as the warm cell. But if
  A1 also writes a *failure* row for `purplepag.es` (Phase 1
  EoseNoMatch), A2's Phase 1 may *exclude* `purplepag.es` if its
  warm threshold trips. Plan doesn't say. **Test**: after A1
  priming, assert the operator-pinned `purplepag.es` survives
  Phase 1 in A2 regardless of its EoseNoMatch decrement (this is
  the `relay_is_operator_pinned` invariant at
  `crates/nmp-planner/src/selection.rs:94` — must hold for
  warm-filtered Phase 1 too).

---

## E. Alternative designs

Two alternatives the plan dismisses or doesn't consider, with concrete
"do Y because Z" arguments:

### E.1 Hint-driven Phase 2 vs LogicalInterest-per-candidate

Plan §0 Q9 picks "one new `LogicalInterest` per Phase 2 candidate"
because per-relay outcome attribution requires distinct `sub_id`s. The
**alternative**: ONE LogicalInterest with `hints: Vec<RelayHint>`
populated with all Phase 2 candidates, relying on W7's per-relay
routing to produce N `RelayEntry`s. The wire-emitter (whose precedent
is the planner's existing per-relay partition at
`crates/nmp-planner/src/compiler/partition/`) assigns each
`RelayEntry` a distinct `sub_id` because it deduplicates per
`(relay, canonical_filter_hash)`. The score-update seam still
attributes via `sub_id → relay_url` from `wire.subs`.

**Why this is better**: one registry slot per claim instead of N. The
existing `OneshotApi` dedup invariant (one `(scope, shape)` → one
slot) is preserved; the Phase 2 expansion fans out through hints, not
through interest multiplicity. The plan's approach inflates
`oneshot.in_flight()` by `MAX_EXPANSION_CONCURRENCY` per claim,
which conflicts with `MAX_DISCOVERY_CONCURRENCY = 2` at
`crates/nmp-core/src/kernel/discovery.rs:65` — see issue 4 in §F.

**Why the plan rejects it (impl-plan.md:28)**: "per-relay outcome
attribution requires distinct `sub_id`s". That's true at the **wire
sub_id** level, not at the **interest_id** level. The partition
already emits distinct `sub_id`s per relay; the plan's reasoning
conflates the two.

### E.2 Score-on-events-not-EOSE

Plan W3 increments `successes` on EVENT match (`Hit`) and `failures`
on EOSE-no-match. **Alternative**: don't decrement on EOSE-no-match
at all. EOSE-no-match for a niche-event claim is ambiguous: the
relay might be perfectly fine, just doesn't store *this* event.
Decrementing turns a single "I don't have this event" into a relay
demerit; across many niche claims, this lowers good-but-narrow
relays below `WARM_THRESHOLD` and removes them from Phase 1
forever.

**Why this might be better**: Gigi's outbox is `wss://relay.dergigi.com`
(per spec §1). If she publishes 50 articles over a year and the
user reads 10 of them, the relay sees 40 EoseNoMatch decrements vs
10 success increments — `weight = 10/51 ≈ 0.196`, below 0.40. The
relay drops out of Phase 1 *despite* being her single most reliable
relay.

**Counter**: pure `successes`-only is just "did relay R ever deliver
an event for author A". That's robust but cold-starts forever. A
middle path is to decrement only on **`Failed` (socket-level
failures)**, not on EOSE-no-match. The plan's table at
impl-plan.md:238-243 collapses these; **recommend splitting them**:
`Hit` → +1 success, `EoseNoMatch` → +0 (neutral), `Failed` → +3
failures (large decrement). The 0.40 threshold then admits any
relay with ≥1 success-without-socket-failure, which is the actual
property we want for Phase 1.

This is the **single biggest design change** the plan should
adopt before Phase 3.

---

## F. Convergence verdict

**Approve with changes** — the architecture is sound, the workstream
breakdown is realistic, and the doctrine analysis is largely
correct. But four items must land as plan amendments before Phase 3
starts:

1. **W3 retarget**: the failure-callback file is `kernel/requests/relay_lifecycle.rs::relay_failed`, **not** `kernel/relay_transport.rs::FailedAfterRetries`. Fix impl-plan.md:230 and the linked test plan.
2. **`oneshot.in_flight()` shared-counter conflict** with `MAX_DISCOVERY_CONCURRENCY = 2` at `crates/nmp-core/src/kernel/discovery.rs:65`. Either (a) Phase 2 expansion uses hints on one interest (E.1 above), or (b) introduce a separate per-claim concurrency tracker that doesn't bump `oneshot.in_flight()`. Silent shared-counter contention is a real bug.
3. **`pending_claims: Vec<PendingClaim>`** is wrong shape. Every EOSE/EVENT frame hits the ingest hot path; an O(N) scan is wrong. Use `BTreeMap<InterestId, PendingClaim>` + reverse `BTreeMap<sub_id, InterestId>` populated via the existing `register_planner_wire_frames` bridge (the same path that fills `pending_discovery_oneshots`).
4. **§0 Q3 reconnect-backoff justification** is factually wrong (`RELAY_RECONNECT_DELAY_INITIAL = 3 s`, not ≥8). Rewrite against actual constants or against measured data; the **values are fine**, just the rationale.

Issues 5–8 are amendments that can land alongside Phase 3 commits but
should be tracked: A.4 race contract, A.5 Arc-vs-`&self` for score
lookup, B.4 `OnceLock<bool>` cache, B.5 Article VII citation, C E12–E16
edge cases, and E.2 score-on-Failed-only.

Do **not** block. The plan is the strongest workstream breakdown I've
reviewed in this repo; the four required fixes are concrete and small.
The implementer can adopt them as a single amendment commit on this PR
before W1 begins.
