---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - declared-projections
  - adr-0053-host-declared
  - snapshot-registry
  - chirp-consumed-projections
supersedes: []
related_claims: []
source_lines:
  - 6379-6455
captured_at: 2026-06-13T21:45:23Z
---

# Episode: Host-declared projections shipped with three debts: silent footgun, unbuilt drift gate, unenforced init-only invariant

## Prior State

ADR-0053 (host-declared projections, PR #1339) merged to master. ADR text claims: (1) empty declared set = emit everything is a safe default analogous to relay interest; (2) a drift-protection gate ensures shells' declared set matches what they decode; (3) declaration is init-only, written once before first real frame. Chirp declares all 18 built-in projections — narrowing nothing.

## Trigger

Post-hoc opus review found: (1) empty=everything is a silent-perf footgun — an app forgetting declaration gets identical correct behavior but full 4 Hz firehose with zero warning/logs; the ADR's promised debug_assert/lint was never built. (2) The 'drift protection' gate the ADR claims in Consequences does not exist — declared set is hand-maintained in declared_projections.rs, not generated from registry; narrowing it would silently dark-screen without any test catching it. (3) declare_consumed_projections has no started-guard — callable mid-session despite ADR claiming init-only.

## Decision

Fix-forward in a new PR: (a) Enforce declaration at the nmp-defaults builder layer (strict for app-facing surface, permissive for kernel-internal Rust consumers). (b) Add one-time tracing::warn! on first emit with empty declared set in NmpApp composition root. (c) Generate CHIRP_CONSUMED_BUILTIN_PROJECTIONS from the codegen registry (single source of truth, bidirectional pin: declared set equals decoded set). (d) Add debug_assert(!started) in nmp_app_declare_consumed_projections to enforce init-only. (e) Amend ADR text to match what's actually built.

## Consequences

- Currently zero shipping apps benefit from host-declared projections — Chirp declares all 18 built-ins, so relay_diagnostics still ships 4×/sec. The real perf lever is incremental emission, not static declaration.
- Hand-maintained declared list is a latent dark-screen hazard exactly when someone narrows it — must be generated from registry
- ADR-0053's text over-claims (drift gate described as existing but unbuilt, init-only invariant stated but unenforced) — ADR must be amended to match reality before the fix-forward closes the gap
- Two ADR-0053s exist on master (host-declared + web-persistence) — numbering collision must be resolved

## Open Tail

- P1: Build the drift gate (generate declared set from codegen registry)
- P1: Enforce non-empty declaration at builder layer + add warn
- P3: Pin init-only invariant with started-guard debug_assert

## Evidence

- transcript lines 6379-6455

