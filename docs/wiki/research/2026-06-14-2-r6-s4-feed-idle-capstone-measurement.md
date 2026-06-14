---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 10560-10609
agent_attribution: sonnet (implement R6-S4 agent)
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "R6-S4 feed-idle capstone measurement: two-phase idle benchmark proving 97.6% frame-byte reduction with 4 pre-registered PASS/FAIL gates all passing"
captured_at: 2026-06-14T17:20:21Z
---

R6-S4 feed-idle capstone measurement: two-phase idle benchmark proving 97.6% frame-byte reduction with 4 pre-registered PASS/FAIL gates all passing

---

User: [Agent task result: Agent "Implement R6-S4 capstone measurement" completed]
Here is the complete report:

---

SUMMARY: ADR-0055 R6-S4 feed-idle capstone implemented — empirical whole-product idle win measured and gated.

ANALYSIS: The critical gap: R3-S5 measured 18%/68.8% row suppression but never registered `nmp.feed.home` (the dominant ~41KB/tick payload). The harness's existing `nmp_app_new()` path had no op_feed wiring at all. This step adds `nmp-defaults` as a binary dependency, registers the feed via `register_op_feed_defaults` (with viewer pubkey + self-inclusion for follow predicate), and injects 120 kind:1 events from the viewer pubkey to fill the 80-card window. The two-phase measurement (8 idle ticks each) then captures the real whole-product win.

ACTIONS:
- Added `nmp-defaults` to `[dependencies]` in `Cargo.toml` (the binary needs it, not just dev tests)
- Created `s7_feed_oracle.rs` (179 LOC) — fail-closed byte-identity oracle; replays ALL Phase B raw frames (settle baseline + 8 idle ticks = 11 frames) through MiniProjectionCache; `nmp.feed.home` must be in the reconstruction (it IS from the first full-baseline tick); ALLOWED_ABSENT = `["claimed_event_embeds", "nip46_onboarding"]`
- Created `s7_feed_gates.rs` (258 LOC) — 4 hard PASS/FAIL gates + JSON measurements block + report notes
- Created `s7_feed_idle.rs` (387 LOC) — two-phase measurement driver with separate `CaptureState` (settle/idle/probe window markers), false-resend probe using STRANGER_PUBKEY events
- Wired S7 `FeedIdle` scenario into `main.rs` (enum, name, parse, default duration, dispatch)

RESULTS: All 4 hard gates pass on two consecutive runs:

| metric | Phase A (incremental OFF) | Phase B (incremental ON) |
|--------|--------------------------|--------------------------|
| idle total p50 | 45,440 B | **1,104 B** |
| idle total p99 | 45,440 B | 1,104 B |
| feed payload p50 | 41,112 B | **0 B (omitted)** |
| serialize_us p50 | 3,581–3,712 µs | 2,411–2,514 µs |
| frames with feed | 8/8 | 0/8 |

Gate 1 `idle_feed_bytes_omitted`: 8 frames ≥ 1 — PASS
Gate 2 `p50_frame_bytes_incremental_lt_baseline`: 1,104 ≤ 45,439 — PASS
Gate 3 `byte_identity_oracle`: 17 keys byte-identical, 0 hard failures, fail-closed — PASS
Gate 4 `false_resend_rate`: 0/20 out-of-window events triggered false re-emit — PASS

HEADLINE: **97.6% idle total-frame-byte reduction** (44,336 B saved per tick). This is the REAL whole-product win R3-S5 could not show.

STATUS: Branch `adr-0055-r6-s4-feed-capstone` pushed. `cargo test -p nmp-testing` green (all tests pass). `doctrine_lint_smoke` green (60 tests). File-size gate exit 0. All new files under 500-LOC ceiling (387/258/179 LOC). GitHub API rate limit prevented `gh pr create` — PR needs to be created at: `https://github.com/pablof7z/nostr-multi-platform/compare/adr-0055-r6-s4-feed-capstone`

CAPTURE: The byte-identity oracle design for R6-S4 differs from R6-S3: it uses the FULL Phase B stream (settle + idle = 11 frames, not just idle frames) so the first baseline tick seeds the reconstruction. `nmp.feed.home` is NOT in the ALLOWED_ABSENT whitelist — it must be present in the reconstruction from tick 1 or the oracle hard-fails. The settle tick captures the first full-frame baseline even with incremental ON (because `declare_incremental_apply` triggers a rebaseline on the first configure). serialize_us does NOT regress (ratio 0.67–0.68 < 1.20 threshold).

NEXT: Create the PR at the GitHub URL above; the Opus reviewer should scrutinize: (1) the oracle's two-phase frame capture design and whether 11 frames is sufficient evidence; (2) whether the IDLE scenario framing is honest enough (it is — every notes line says "IDLE/static-feed scenario only"); (3) whether the false-resend gate logic is sound (STRANGER_PUBKEY is not in follow set → engine predicate gates events out → content unchanged → byte-equality gate fires no Changed → zero false resends).

STORY EXPLANATION:
1. The mandate was to register op_feed in the harness and measure the real idle win that R3-S5 couldn't show because the feed projection was never wired
2. Added `nmp-defaults` as a binary dependency so the ffi-stress binary can call `register_op_feed_defaults` directly in Rust
3. The feed seeding strategy uses `VerifiedEvent::from_raw_unchecked` to inject 120 kind:1 events from VIEWER_PUBKEY (the active account), exploiting self-inclusion so the follow predicate accepts them
4. The two-phase capture structure separates oracle frames (all frames from phase start) from metric frames (idle window only) using explicit index markers in `CaptureState`
5. The oracle fail-closed design correctly places `nmp.feed.home` as NOT whitelisted — the first Phase B settle tick is the full baseline that seeds the reconstruction, so its presence is guaranteed
6. The false-resend probe injects STRANGER_PUBKEY events after the idle window and asserts zero feed re-emits — proving the byte-equality gate doesn't over-invalidate on out-of-window ingest
7. All 4 hard gates pass with consistent numbers across two runs: 45,440B baseline → 1,104B incremental (97.6% reduction, 44,336B saved per idle tick)
8. The honesty framing is explicit throughout: IDLE/static-feed scenario, row-deltas are Option B deferred post-v1, serialize_us informational only

COMPLETED: R6-S4 capstone proves 97.6 percent idle frame reduction with feed registered
