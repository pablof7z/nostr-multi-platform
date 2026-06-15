---
type: research-record
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
source_lines: 2388-2412
agent_attribution: a4c8fbca3a3d0617f
has_preregistered_criteria: false
has_method: true
has_structured_report: true
characterization: "Branch comparison experiment distinguishing flake from real CI regression: PR branch 0/8 vs master 2/5 wasm-pack test passes; Verdict: REAL regression from PR #1436"
captured_at: 2026-06-15T04:33:11Z
---

Branch comparison experiment distinguishing flake from real CI regression: PR branch 0/8 vs master 2/5 wasm-pack test passes; Verdict: REAL regression from PR #1436

---

User: [Agent task result: Agent "Repro + fix web feed CI failure on PR branch" completed]
## Summary

**Verdict: REAL regression from PR #1436, now fixed and pushed.**

### Evidence (flake vs real)
With authentic `wasm-pack` builds run locally exactly as CI does:
- **PR branch: 0/8 passed** — failed consistently at the profile-resolution assertions (`feed.spec.ts` lines 96/104/120: Alice's name, Bob's attribution, avatar).
- **master: 2/5 passed** (the rest flaked only at line 131, the deepest assertion — a slow-render tail flake on my slower-than-CI macOS; master never failed early like the PR).

This matches CI (master green, PR red) and is not a flake.

### Root cause
The profile-claim → registry migration is correct in `nmp-core`, but on the web (single-threaded wasm worker) it surfaced an **unbounded UI loop**. The wasm dispatch claim/release arm pushed a fresh snapshot frame on **every** `claim_profile`/`release_profile`. The web feed's SolidJS `<For>` rebuilds its rows on each snapshot → remounts the registry `<NostrAvatar>`/`<NostrProfileName>` components → their `onMount`/`onCleanup` re-dispatch claim/release → another snapshot → loop. Worker instrumentation proved it: **170k+ snapshot frames, `handle_json` called 16k+ times alternating claim/release**, OOM-crashing the renderer (or starving it so names/avatars never resolved in time).

### The fix (commit `a97bb8f65`, pushed to the PR branch)
`crates/nmp-wasm/src/runtime.rs` — the claim/release dispatch arm now ACKs with `ActionAccepted` only and **pushes no snapshot**. Claim/release are refcount bookkeeping carrying no new user-visible data; the resolved kind:0 arrives via the relay-pool ingest sink, which pushes its own snapshot. This removes the loop's engine. Mirrors the native actor.
- Added native regression guards (`claim_no_snapshot_tests`) asserting claim/release emit `ActionAccepted` with no `UpdateBytes`.

### Confirmation
- `feed.spec.ts`: **10/10 pass**; full web Playwright suite **3/3**; vitest **42/42**.
- `cargo test -p nmp-core`, `-p nmp-wasm`, and `doctrine_lint_smoke`: **all green**. File-size gate passes.

### Note on coordination
While I worked, the coordinator's agent independently pushed `524909a02` (gate the `relay_connected_url` indexer reprobe to genuine reconnects — the same connect-time churn I'd also identified). That fix is complementary (removes connect-time recompile churn) but did **not** alone make the test pass (verified 0/6); the claim/release snapshot suppression is the decisive fix. My commit builds on top of it. I did not merge the PR.
