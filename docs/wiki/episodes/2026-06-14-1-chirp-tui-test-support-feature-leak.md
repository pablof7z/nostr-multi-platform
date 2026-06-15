---
type: episode-card
date: 2026-06-14
session: 418d555f-8e77-4e56-8166-93d1fef9cfce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/418d555f-8e77-4e56-8166-93d1fef9cfce.jsonl
salience: root-cause
status: superseded
subjects:
  - chirp-tui
  - nmp-core-test-support
  - adr-0055-oracle
supersedes:
  - 2026-06-14-1-test-support-feature-leaks-adr-0055
related_claims: []
source_lines:
  - 35-67
  - 186-261
  - 434-440
  - 449-487
  - 529-545
captured_at: 2026-06-14T21:12:13Z
---

# Episode: chirp-tui test-support feature leak crashes app at boot

## Prior State

chirp-tui enabled `features = ["test-support"]` on nmp-core and nmp-ffi in its runtime `[dependencies]`, compiling the test-only ADR-0055 Rung-1 projection-rev oracle (`cfg(any(test, feature="test-support"))`) into the shipping binary. The feature is documented "Never enable in production builds."

## Trigger

User reports chirp-tui is completely unresponsive on launch. Reproduction in tmux reveals the kernel actor panics at `kernel_oracle.rs:33` on a `claimed_events` StaleStamp violation (the projection's cache unit changed on the declaration absent→present-empty boot transition but rev didn't advance). The panic kills the actor mid-render, leaving the terminal frozen at the last drawn frame — the app was crashing, not hanging.

## Decision

Relocate `test-support` from runtime `[dependencies]` to `[dev-dependencies]` in chirp-tui/Cargo.toml. Runtime only needs the default `native` C-ABI surface (`nmp_app_ack_action_stage` etc.); the injectors and `spawn_actor` are used exclusively by `#[cfg(test)]` files. Deliberately did NOT paper over the underlying stale-stamp with a naive rev bump — the correct fix belongs to the ADR-0055 Rung-3 declaration→presence machinery being actively designed.

## Consequences

- App boots into live home feed and fully responds to input (?→Help, Esc→close, /→command palette)
- cargo test -p chirp-tui still links the oracle under dev-dependencies (test-support-gated round-trip test passes)
- doctrine_lint_smoke passes (60/0)
- The underlying claimed_events declaration-transition stale-stamp is filed as issue #1430 — a real ADR-0055 Rung-3 latent but harmless in production since the oracle is now absent from the shipping binary

## Open Tail

- Suggested doctrine-lint rule forbidding app crates from enabling test-support in runtime [dependencies] to prevent recurrence — not yet implemented
- Issue #1430: claimed_events absent→present-empty transition stale-stamp deferred to Rung-3 owner for proper declaration→presence fix

## Evidence

- transcript lines 35-67
- transcript lines 186-261
- transcript lines 434-440
- transcript lines 449-487
- transcript lines 529-545
