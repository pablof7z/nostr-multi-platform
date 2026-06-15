---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 10867-10913
agent_attribution: ac4b61441e369b683
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Release-build A/B idle-jank measurement: refutes hypothesis that feed-omission stops idle timeline body re-eval; .equatable() boundary is the actual shield, body-evals/sec = 0 in both arms"
captured_at: 2026-06-14T20:53:43Z
---

Release-build A/B idle-jank measurement: refutes hypothesis that feed-omission stops idle timeline body re-eval; .equatable() boundary is the actual shield, body-evals/sec = 0 in both arms

---

User: [Agent task result: Agent "Release on-device/sim jank A/B measurement" completed]
The voice-notify endpoint is not running (404), but the task is complete. Here is the report.

---

## Release-build A/B idle-jank measurement — ADR-0055 Rung 6 (issue #1415)

Posted: https://github.com/pablof7z/nostr-multi-platform/issues/1415#issuecomment-4702646802

### What I did
Built Chirp in **RELEASE** (core via `cargo build --release ... --target aarch64-apple-ios-sim`, app via Xcode `-configuration Release ARCHS=arm64` — confirmed linking the `release/` Rust archive), added minimal spike instrumentation, ran the identical idle scenario with incremental apply ON (default) vs OFF (commented out the declare call at KernelBridge.swift:69, rebuilt Release), then reverted everything.

### The numbers (idle, iOS sim iPhone 16, arm64, Xcode 26.5)

| metric (idle) | ON (default) | OFF |
|---|---|---|
| `nmp.feed.home` in `changedKeys` | FALSE 149/149 ticks | TRUE 52/52 ticks |
| **timeline body-evals/s** | **0** (all 31 windows) | **0** (all windows) |
| timeline `==` compares/s | ~13.8 | ~12 |
| apply µs/tick | median 183, mean 196 | median 167, mean 200 |

### The answer to the decisive question
**Refuted (in part).** The R6-S5 hypothesis was that *feed-omission* is what stops the idle timeline body re-eval. The data shows the home-feed timeline body does **not** re-evaluate on idle — but **`.equatable()` at HomeFeedView.swift:147 is the load-bearing shield, not the `changedKeys` omission.** In the OFF arm, `typedHomeFeed` is reassigned every tick (`@Published` fires, `==` is invoked ~12/s) yet body-evals are still **0**, because `roots` is unchanged so `.equatable()` short-circuits. Omission is a real, redundant second layer (it cuts `@Published` fan-out and frame bytes — R6-S4's 97.6% win), but it is not what stops *this* body recompute.

Bonus finding: the kernel is **not** a blind 4Hz pump at true idle — it ticks only when something changes (relay_diagnostics/nip46_onboarding churn); deep-idle ticks were ~11s apart.

### Caveats (honest)
- **Sim, not device.** Body-eval *counts* are portable (0-vs-nonzero holds); absolute µs and any frame/scroll/hitch claim are not representative — needs on-device Instruments.
- The two arms rendered different feed sizes (70 vs 1 card) from live-relay timing; doesn't affect the body-eval result (0 in both) but makes apply/render *cost* non-comparable.
- `log stream` for `.debug` stalled repeatedly on this sim; OFF data was captured by bumping spike logs to `.info` and reading the persistent store via `log show`.

### codex verdict
"Supports 'idle timeline body recomposition is shielded,' not 'idle jank is fixed'; the `.equatable()` boundary is the shield, omission prevents the reassignment but reassignment alone doesn't force body re-entry while `==` returns true. End-to-end jank still needs frame/scroll instrumentation."

### One-line final answer
On Release, the idle home-feed timeline body does not re-render — but `.equatable()` is the real shield; incremental-apply omission is a valuable-but-redundant second layer, and "idle jank fixed" is not yet provable without on-device frame data.

### Cleanup
- Spike instrumentation fully reverted. `git status --porcelain` is **empty** (pristine tracked tree). The new `SpikeDiag.swift` was deleted; the 3 source edits (KernelBridge/KernelModel/HomeFeedView), `Cargo.lock`, and `project.pbxproj` were restored. Remaining `SpikeDiag` strings exist only under gitignored `ios/DerivedData/` (build artifacts).
- No code PR opened. Did **not** propose a standing diagnostic — the existing `NMP_PERF swift_apply` line already carries `apply_us`; adding a permanent `changedKeys`/body-eval probe isn't worth the thin-shell noise given the finding (the `.equatable()` boundary, not the projection slot, is the thing to watch).

Relevant files (absolute):
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-ac4b61441e369b683/ios/Chirp/Chirp/Features/HomeFeedView.swift` (the `.equatable()` boundary at line 147 — the actual shield)
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-ac4b61441e369b683/ios/Chirp/Chirp/Bridge/KernelModel.swift` (`changedKeys` gating at lines 778–822)
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-ac4b61441e369b683/ios/Chirp/Chirp/Bridge/KernelBridge.swift` (line 69, `nmp_app_declare_incremental_apply` — restored live)

One process note: the `localhost:8888` voice-notify endpoint returns 404 (server not running), so the mandated voice notifications could not be delivered this session.
