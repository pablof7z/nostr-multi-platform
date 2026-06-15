---
type: episode-card
date: 2026-06-14
session: 418d555f-8e77-4e56-8166-93d1fef9cfce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/418d555f-8e77-4e56-8166-93d1fef9cfce.jsonl
salience: root-cause
status: active
subjects:
  - chirp-tui
  - adr-0055-oracle
  - test-support-leak
  - claimed-events
supersedes:
  - 2026-06-14-1-chirp-tui-freeze-caused-by-test
related_claims: []
source_lines:
  - 36-67
  - 186-216
  - 232-262
  - 368-440
  - 527-545
captured_at: 2026-06-14T21:37:19Z
---

# Episode: chirp-tui freeze from test-support oracle shipped into production binary

## Prior State

chirp-tui enabled features = ["test-support"] on nmp-core and nmp-ffi in runtime [dependencies]. This compiled the ADR-0055 Rung-1 projection-rev oracle (gated cfg(any(test, feature="test-support"))) into the shipping binary. The oracle panics on any missed rev-stamp. At boot, the claimed_events projection transitions from absent to declared-present-empty without a rev bump, tripping a StaleStamp violation that crashes the kernel actor mid-render, leaving the terminal frozen at the last drawn frame.

## Trigger

User reported chirp-tui appears completely frozen and unresponsive. Haiku agent reproduced in tmux, identified panic at kernel_oracle.rs:33 (claimed_events StaleStamp). Investigation confirmed nmp_app_ack_action_stage (the only nmp-ffi symbol chirp-tui runtime needs) is gated by native (default), not test-support — runtime test-support has no legitimate consumer; only #[cfg(test)] files use spawn_actor/inject symbols.

## Decision

Relocate test-support from runtime [dependencies] to [dev-dependencies] in chirp-tui/Cargo.toml. Runtime builds no longer compile the oracle; cargo test still links it (fails loud on real violations). The underlying claimed_events declaration-transition stale-stamp was filed as #1430 but deliberately not patched — it is a real ADR-0055 Rung-3 latent that the Rung-3 owner should address in the declaration→presence machinery; a naive bump would mask the oracle's signal.

## Consequences

- Shipping chirp-tui binary no longer carries the test-only oracle; boot-time StaleStamp panic eliminated
- App boots into live home feed and responds to input (verified under real PTY with ?, Esc, / keystrokes)
- cargo test -p chirp-tui still compiles with oracle active (dev-dependencies re-pin preserves cfg(test) round-trip test and spawn_actor access)
- Issue #1430 tracks the genuine claimed_events absent→present-empty declaration-transition stale-stamp for ADR-0055 Rung-3 resolution
- No CI/doctrine-lint guard prevents future app crates from re-enabling test-support in runtime deps — recurrence risk flagged

## Open Tail

- ADR-0055 Rung-3 owner needs to fix the declaration-transition stale-stamp (#1430) before the oracle can be safely re-enabled in production
- A doctrine-lint rule forbidding app crates from enabling test-support in [dependencies] (vs [dev-dependencies]) would prevent recurrence

## Evidence

- transcript lines 36-67
- transcript lines 186-216
- transcript lines 232-262
- transcript lines 368-440
- transcript lines 527-545
