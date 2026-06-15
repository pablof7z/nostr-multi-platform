---
type: episode-card
date: 2026-06-14
session: 418d555f-8e77-4e56-8166-93d1fef9cfce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/418d555f-8e77-4e56-8166-93d1fef9cfce.jsonl
salience: root-cause
status: superseded
subjects:
  - chirp-tui
  - adr-0055-oracle
  - test-support-feature-leak
supersedes:
  - 2026-06-14-1-chirp-tui-test-support-feature-leak
related_claims: []
source_lines:
  - 1-79
  - 186-261
  - 300-439
  - 449-470
  - 527-545
captured_at: 2026-06-14T21:27:55Z
---

# Episode: chirp-tui freeze caused by test-only oracle compiled into production binary

## Prior State

chirp-tui enabled features = ["test-support"] on nmp-core and nmp-ffi in its runtime [dependencies], compiling the ADR-0055 Rung-1 projection-rev oracle (cfg(any(test, feature="test-support"))) into the shipping binary. The oracle panics on any missed rev-stamp. At boot, the claimed_events projection's declaration absent→present-empty transition triggers a StaleStamp violation, crashing the kernel actor mid-render and freezing the terminal on the last drawn frame.

## Trigger

User reported chirp-tui completely frozen and unresponsive on launch. Haiku agent reproduced in tmux, captured the panic at kernel_oracle.rs:33 (claimed_events StaleStamp). Opus traced the feature-gating chain and confirmed nmp_app_ack_action_stage (the only runtime FFI symbol) is gated by native, not test-support — the runtime binary has zero legitimate need for test-support.

## Decision

Relocate test-support from runtime [dependencies] to [dev-dependencies] in chirp-tui's Cargo.toml. The underlying claimed_events declaration-transition stale-stamp was NOT patched with a naive rev-bump — it was filed as issue #1430 for the ADR-0055 Rung-3 owner, since a bump would mask the oracle's signal and the correct fix belongs in the declaration→presence machinery.

## Consequences

- Production chirp-tui binary no longer contains the panic-oracle; app boots into live home feed and responds to input
- cargo test still links test-support (dev-deps), so the oracle remains active under test and the cfg(test) round-trip test still compiles and passes
- Issue #1430 filed for the latent claimed_events StaleStamp — a real Rung-3 invariant gap that is harmless in prod (oracle off) but must be resolved architecturally
- Recurrence risk identified: no CI/doctrine-lint rule prevents app crates from enabling test-support in runtime [dependencies]; a guard was proposed but not implemented

## Open Tail

- ADR-0055 Rung-3 owner needs to resolve #1430 (declaration-transition stale-stamp) in the projection presence machinery
- No CI guard yet to prevent future test-support leaks into app runtime deps

## Evidence

- transcript lines 1-79
- transcript lines 186-261
- transcript lines 300-439
- transcript lines 449-470
- transcript lines 527-545
