---
type: research-record
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
source_lines: 3530-3557
agent_attribution: a34b9daeb61f939cf
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Web feed E2E verification of PR #1448 (primal→app relay + kind:10002 probe additive to app relays): tested that recompile.rs change doesn't cause feed regression — verdict PASSES"
captured_at: 2026-06-15T10:55:12Z
---

Web feed E2E verification of PR #1448 (primal→app relay + kind:10002 probe additive to app relays): tested that recompile.rs change doesn't cause feed regression — verdict PASSES

---

User: [Agent task result: Agent "Verify web feed test on #1448" completed]
Main checkout is still on master, untouched. My isolated worktree is at the PR head sha 8e8056da4.

## Verdict: PR #1448 PASSES — no web-feed regression. Safe to merge.

I built the wasm and ran the web Playwright feed test exactly as CI's path-filtered "wasm facade and web app" job does, in an isolated git worktree (`/private/tmp/nmp-pr1448-verify`, detached HEAD at PR head `8e8056da4`). The main checkout was never touched (still on `master`).

### Build
- `wasm-pack build --target web apps/chirp/nmp-app-chirp-web --out-dir ../../../web/chirp/public/nmp-wasm` — EXIT 0 (used Homebrew LLVM clang via `CC_wasm32_unknown_unknown` + `AR_wasm32_unknown_unknown=llvm-ar`, since Apple clang lacks the wasm32 backend). `recompile.rs` compiled cleanly into the wasm.
- `npm ci`, `npm run build`, `npx playwright install chromium` — all EXIT 0.

### Test results — `feed.spec.ts`
- Run 1: **1 passed** (3.6s)
- Run 2: **1 passed** (3.1s)
- Run 3: **1 passed** (2.7s)
- Full CI suite (boot + feed + inspector): **3 passed** (4.1s)

No flakiness across 3 dedicated feed runs plus the full-suite run.

### Specific regression checks (all clear)
- **(a) No recompile/snapshot loop wedging the UI.** The fixture-relay REQ log shows exactly **two distinct `mailbox-probe-*` REQs per run** (one for the viewer, one batched for the two follows), each emitted **once** — no runaway/duplicate mailbox-probe storm. The new `probed_mailboxes` dedup still holds even though the probe target is now `indexer_relays ∪ app_relays`. Runtime reached status "running" and the test completed in ~2-3s (a recompile loop would have hung to the 150s timeout).
- **(b) Feed and avatars render.** All six feed assertions passed: note content, resolved kind:0 display name, reply attribution badge, `.post .nostr-avatar` visible, and the inline `nostr:npub` mention chip resolved.
- **(c) No new console panic.** Scanned all run logs for panic/RuntimeError/Uncaught/abort/infinite-loop — **none found**; no `error`/`failed` lines.

### On the change itself
The new probe correctly routes through `auth_gate.partition(probe_frames)`. In this fixture-relay setup the relay is live (not AUTH-paused), so probes pass through and the kind:10002 REQs land normally — confirming the auth-gating path does not block or duplicate probes on a healthy relay.

Artifacts are kept in `/private/tmp/nmp-pr1448-verify/` (wasmbuild.log, webbuild.log, feed-run1/2/3.log, full-suite.log). Nothing was merged.
