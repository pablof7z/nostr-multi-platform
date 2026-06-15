---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 10640-10707
agent_attribution: a6b8549e3fbef8781
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Opus review of R6-S4 capstone: REQUEST-CHANGES on false-resend gate testing trivial out-of-follow-set case instead of real out-of-window over-invalidation risk; 97.6% headline reproduced and honestly framed"
captured_at: 2026-06-14T20:53:43Z
---

Opus review of R6-S4 capstone: REQUEST-CHANGES on false-resend gate testing trivial out-of-follow-set case instead of real out-of-window over-invalidation risk; 97.6% headline reproduced and honestly framed

---

User: [Agent task result: Agent "Review R6-S4 capstone (local git)" completed]
Voice server is returning 404 (endpoint unreachable in this environment), but I attempted the notifications as required. Here is the review.

---

📋 SUMMARY: Review of ADR-0055 R6-S4 feed-idle capstone — the headline reproduces and is honestly framed, but the false-resend gate tests the trivial case, not the real over-invalidation risk.

🔍 ANALYSIS:

**VERDICT: REQUEST-CHANGES** (one methodology gap; everything else is sound)

**(a) The false-resend gate tests the WRONG thing — confirmed.**
The probe injects `STRANGER_PUBKEY` events. A stranger is rejected by `follow_set.predicate()` and **never enters the feed engine at all**. The typed emit closure (`op_feed_defaults.rs:294-340`) calls `engine.snapshot(&FeedRequest::default())` then byte-compares — but if the event never reached the engine, the snapshot is trivially byte-identical regardless of whether the byte-equality gate works. This gate would pass even with a broken `should_emit`. The gate's own note admits it: *"non-followed pubkey, never changes the feed."* That is the easy out-of-follow-set case, not the out-of-window risk.

The real over-invalidation risk this capstone must prove absent: a **followed author's event that falls outside the visible 80-card window** (`DEFAULT_FEED_WINDOW_LIMIT = 80`, `crates/nmp-feed/src/types.rs:5`). Such an event passes the predicate, mutates the engine's internal card set, but `snapshot(default 80-window)` still returns the same top-80 → byte-identical → must be omitted. Only that case exercises the byte-equality gate as the suppressor.

Two details that make the fix cheap and the gap glaring:
- The harness already seeds **120** VIEWER events into an **80**-card window, so ~40 followed out-of-window events already exist — the data for the real test is present.
- The probe stranger events use `base_ts + 200_000` (the *newest* timestamps). Had they been from a followed author they'd land at the *top* of the window and correctly force a re-emit. The required probe is the opposite: a **followed** author event with an **old** `created_at` (e.g. `base_ts - 1`), older than the 80th card → assert no feed re-emit.

**Requested change:** in the Phase-B probe, ingest a *followed* (VIEWER or an added followed pubkey) event older than the 80th visible card and assert `false_resend_count == 0`. Keep the stranger probe too (it's a valid predicate check), but it cannot stand alone as the over-invalidation proof.

**(b) Headline reproduces and is honestly framed — confirmed.**
My run: baseline p50 = 45,440 B → incremental p50 = 1,104 B = **97.6% reduction**; feed payload 41,112 → 0 (omitted on 8/8 idle ticks); all 4 gates pass; oracle 17 keys byte-identical, 0 hard failures; false-resend 0/20. Matches the reported numbers.
- It is a **debug** build (my `serialize_us` p50: 4,098 OFF / 2,718 ON — same order as the reported 3,600/2,460; debug-inflated vs the ~129µs release encode from #1415). Correctly labeled informational, explicitly not gated, not presented as release latency. Good.
- Every notes line qualifies this as the IDLE/static-feed scenario and states a mutating in-window event still re-sends the whole feed (Option B deferred post-v1). No overclaim of general O(changed). Honest framing.

**Gates hard-fail correctly:** `Gate::lte/gte` compute `passed`; `report.finish` sets `passed = Gate::all_pass`; `--fail-on-gate` → `process::exit(2)`. I confirmed exit 0 on the passing run. Note: unlike S2/S3, there is **no CI workflow** wiring `feed-idle --fail-on-gate`, so this is a one-shot measurement artifact, not a standing regression gate (nit; consistent with "capstone measurement").

🔍 ORACLE FIDELITY:
- `nmp.feed.home` is correctly NOT in `ALLOWED_ABSENT` (only `claimed_event_embeds`, `nip46_onboarding`), so the feed must be reconstructed from the Phase-B baseline tick or the oracle hard-fails. Run confirmed 17 keys matched, 0 absent — feed was reconstructed. Fail-closed logic is correct.
- It replays the FULL Phase-B stream (settle baseline + idle = 11 frames) — correct.
- **Fidelity overstatement:** `MiniProjectionCache` faithfully models the steady-state subset of the real `ProjectionCache.generated.swift` — Changed=insert, Cleared=remove, omit=retain. But it does **NOT** model session/epoch rebaseline (D4 `cache.removeAll()`), the `sessionId==0` pass-through, the rev-monotonicity guard (`incomingRev <= cached.rev → skip`), or decode-before-commit. The module doc and the task brief both claim it mirrors "session/epoch rebaseline" — it doesn't, and the scenario never bumps session/epoch (single session, single epoch, monotonic revs, valid payloads), so that path is asserted-by-doc but never exercised. For proving *idle-omission losslessness* the stand-in is adequate; the "mirrors the real ProjectionCache" claim should be narrowed to "mirrors the steady-state Changed/Cleared/retain subset." Nit, not a blocker. 11 frames is sufficient evidence for the idle claim.

🔍 CRATE-BOUNDARY + DEBT:
- `nmp-testing` (Sidecar) depending on `nmp-defaults` (Layer 5) is the allowed direction — a sidecar consuming the composition root to drive a whole-product scenario. No layer inversion. Sound per `docs/architecture/crate-boundaries.md`.
- `VerifiedEvent::from_raw_unchecked` (`crates/nmp-store/src/types/events.rs:187`) is gated `#[cfg(any(test, feature = "test-support"))]` — "NEVER enabled in production builds." Test-only seam, not a production-reachable bypass. Used correctly.

⚡ ACTIONS: `git diff` against master; read all 3 new files + main.rs/Cargo.toml wiring; traced the op_feed predicate + 80-card window + typed-emit byte-equality closure; added a worktree, built and ran `ffi-stress feed-idle` (reproduced 97.6%); ran `cargo test -p nmp-testing` (pass) + `doctrine_lint_smoke` (60 pass); file-size gate exit 0; verified Swift cache semantics vs the oracle stand-in.

✅ RESULTS:
- Headline 97.6% reproduces; honestly framed as idle-only; serialize_us correctly informational/debug.
- 4 gates pass and hard-fail correctly via `--fail-on-gate`.
- Oracle fail-closed logic correct; `nmp.feed.home` not whitelisted; feed reconstructed.
- `cargo test -p nmp-testing` pass, doctrine lint 60 pass, file-size exit 0, all 3 files < 500 LOC (387/258/179).
- Crate dependency direction and the `from_raw_unchecked` seam are both sound.
- One real methodology gap: the false-resend gate proves the trivial case only.

📊 STATUS: Review complete; worktree cleaned up. Empirically verified. Did NOT merge (report-only; GitHub rate-limited anyway).

📁 CAPTURE: R6-S4 (#1415, branch `origin/adr-0055-r6-s4-feed-capstone`, commit 5166139d4): 97.6% idle byte reduction is real and honestly framed, but the false-resend gate uses STRANGER_PUBKEY (gated by predicate → never enters engine) instead of a followed-but-out-of-window event (older than the 80th card; window=80, seeds=120 so data exists). Oracle stand-in mirrors only the steady-state Changed/Cleared/retain subset, not session/epoch rebaseline (never exercised). Verdict: REQUEST-CHANGES.

➡️ NEXT:
1. Add a followed-author out-of-window probe (ingest a VIEWER/followed event with `created_at < base_ts`, older than the 80th card; assert no feed re-emit). Keep the stranger probe as a predicate check.
2. Narrow the oracle module doc to "steady-state subset" or add a session/epoch-bump sub-case to actually exercise rebaseline.
3. Optional: add an S7 CI gate workflow (mirroring s2/s3) if this should be a standing regression guard.

📖 STORY EXPLANATION:
1. The capstone claims a 97.6% idle byte win from feed-gating; I reproduced it exactly by building and running the S7 scenario.
2. The win is real and honestly scoped: every report line qualifies it as the idle/static-feed case and admits a mutating in-window event still re-sends the whole feed.
3. The serialize_us figures are debug-inflated but correctly marked informational and explicitly not gated — no latency overclaim.
4. The four gates compute pass/fail numerically and hard-exit with code 2 under `--fail-on-gate`; I confirmed exit 0 on the passing run.
5. The flaw is the false-resend probe: it uses a non-followed stranger, whom the follow predicate rejects before the feed engine ever sees the event — so it can't exercise the byte-equality gate it claims to prove.
6. The real over-invalidation risk is a *followed* author's event that falls outside the visible 80-card window; the harness already has the data (120 events, 80-card window) so the fix is a few lines.
7. The oracle is fail-closed and correctly forces feed reconstruction, but its cache stand-in mirrors only the steady-state merge subset, not the session/epoch rebaseline the docs claim — that path is never exercised.
8. Crate boundaries and the test-only event-injection seam are sound; the single methodology gap drives the REQUEST-CHANGES verdict.

🎯 COMPLETED: Request changes — false-resend tests trivial case; 97.6% reproduces, honestly idle-framed.
