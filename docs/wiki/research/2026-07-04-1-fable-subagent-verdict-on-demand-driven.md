---
type: research-record
date: 2026-07-04
session: f308bb0b-7b74-4684-9a5b-1fce8ffcab35
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f308bb0b-7b74-4684-9a5b-1fce8ffcab35.jsonl
source_lines: 1007-1028
agent_attribution: a6a0378d22b82e1bc (Fable)
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Fable subagent verdict on demand-driven projection decode design: NOT worth pursuing, citing prior benchmark threshold and code-level invariant analysis"
captured_at: 2026-07-04T11:46:49Z
---

Fable subagent verdict on demand-driven projection decode design: NOT worth pursuing, citing prior benchmark threshold and code-level invariant analysis

---

User: [Agent task result: Agent "Fable: decide on demand-driven projection decode design" finished]
## Verdict: NOT worth pursuing. The eager copy is correct; close this line of inquiry.

No edits made (decide-only, per correction).

### 1. Prior art (#2388) undercuts the premise in both directions

`docs/wiki/research/2026-06-29-1-uniffi-vs-c-callback-byte-push.md` + episode `2026-06-29-1-collapse-uniffi-performance-assumption-unify-to.md`: the measured surcharged weighted-p99 UniFFI-vs-C delta was **~2.1 µs/frame = 0.013% of the 16.67 ms frame budget, 390× below the pre-registered COLLAPSE threshold**, even with a 3× JNI/ARC penalty. Verdict was COLLAPSE — pure UniFFI, zero byte-lane exceptions. Two implications: (a) the "FFI copy is already ≥2 allocs+2 memcpys" cost is *microseconds*, not a dominant cost to amortize around; (b) the benchmark's fairness rule explicitly treated **one host-side consume copy as mandatory** — the callback contract says bytes are valid only for callback duration, so every real host copies once. The eager copy IS that copy.

### 2. Frame contents are already demand-shaped — consumption is ~100% by construction

- **Rung-3 omit-unchanged is live**: `crates/nmp-core/src/kernel/update.rs:413` (`rung3_omit::omit_unchanged`) drops every `Unchanged` row; absence IS Unchanged (`rung3_omit.rs` invariants). Chirp declares the capability at `apps/ios/Chirp/Bridge/KernelBridge.swift:130` (`declareIncrementalApply`, assertion-failure if not).
- **Idle emits nothing**: `crates/nmp-core/src/actor/tick.rs:84` gates emission on `changed_since_emit`; 4 Hz is a max cadence under change, not a drumbeat (confirmed by `v87_d1_startup_tests.rs:186`).
- **Consumed-projection narrowing**: `update.rs:384` retains only host-declared keys; observer-scoped feed keys get `Cleared` then vanish when a view closes.

So every payload byte decoded is a *changed* value the rev-aware merge cache (`KernelModel+Apply.swift`, ProjectionCache) is contractually required to retain. There is no "projections nobody wants" population to skip. The waste is not complexity-level; it isn't even meaningful constant-level.

### 3. Lazy references would be strictly worse

Cached envelopes must outlive their frame (until the next Changed rev for that key, potentially forever). A parent-buffer+offset design pins each entire frame `Data` alive as long as any one of its projections stays cached — mixed lifetimes ⇒ unbounded retained frame buffers, worse memory than today's tight per-payload copies. Plus a wire/contract-adjacent redesign, Swift `Data`-slice aliasing hazards, and zero complexity-class gain (UniFFI already memcpys the whole frame regardless).

The already-scoped `@available(unavailable)` neutering of the slow accessor (deferred, not implemented here) is the complete and final fix.
