---
type: episode-card
date: 2026-06-14
session: 418d555f-8e77-4e56-8166-93d1fef9cfce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/418d555f-8e77-4e56-8166-93d1fef9cfce.jsonl
salience: root-cause
status: superseded
subjects:
  - chirp-tui-deps
  - nmp-core-test-support
  - adr-0055-oracle
supersedes: []
related_claims: []
source_lines:
  - 36-78
  - 186-230
captured_at: 2026-06-14T20:55:49Z
---

# Episode: test-support feature leaks ADR-0055 panicking oracle into chirp-tui production binary

## Prior State

chirp-tui's Cargo.toml `[dependencies]` enabled `features = ["test-support"]` on nmp-core (and nmp-ffi) in runtime (non-test) dependencies. The ADR-0055 projection-rev oracle module is gated behind `cfg(any(test, feature = "test-support"))` and is explicitly documented as zero-cost in production builds — but test-support propagated it into the shipping binary.

## Trigger

User reported chirp-tui appears completely frozen on launch. Agent reproduced in tmux and found the app panics at kernel_oracle.rs:33 — a `StaleStamp` violation on the `claimed_events` projection. The panic kills the process mid-render, leaving the terminal frozen at the last drawn frame with no error visible to the user.

## Decision

The root cause is architectural: `test-support` must not be enabled in chirp-tui's runtime `[dependencies]`. It compiles a test-only correctness gate (ADR-0055 biconditional oracle that panics on any missed rev-stamp) into the production TUI binary, violating the feature's documented contract that production builds carry zero oracle cost. The fix is to remove `features = ["test-support"]` from chirp-tui's runtime nmp-core dependency (and restructure any legitimate test-support usage to test-only dependency or dev-dependency).

## Consequences

- Production chirp-tui binary will no longer include the ADR-0055 oracle and cannot panic on StaleStamp violations at runtime
- Any `nmp_core::testing` items used in chirp-tui (e.g., `spawn_actor` in `feature_snapshot_typed_roundtrip_tests.rs`) must be gated behind `#[cfg(test)]` or moved to dev-dependencies
- The underlying `claimed_events` stale-stamp bug (cache unit changes without rev bump) still exists but will be silent in production — it must be caught and fixed via test builds where the oracle remains active

## Open Tail

- The genuine `claimed_events` StaleStamp — a projection cache unit changed without its revision advancing — must still be fixed at the mutation's write chokepoint so tests pass; the oracle correctly detected it
- Audit other app crates for the same `test-support` leak in runtime `[dependencies]`

## Evidence

- transcript lines 36-78
- transcript lines 186-230
