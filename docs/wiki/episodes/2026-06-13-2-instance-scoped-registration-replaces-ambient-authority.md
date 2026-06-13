---
type: episode-card
date: 2026-06-13
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: active
subjects:
  - instance-scoped-registration
  - ActionModule
  - ACTIVE-WALLET-RUNTIME
  - DispatchHostOp
  - adr-0052
supersedes: []
related_claims: []
source_lines:
  - 5554-5584
  - 5625-5648
  - 5692-5718
captured_at: 2026-06-13T19:44:01Z
---

# Episode: Instance-scoped registration replaces ambient-authority globals (K2/ADR-0052)

## Prior State

ActionModule::execute was a plain fn with no &self, forcing action modules to reach wallet/signer state through ambient-authority globals (ACTIVE_WALLET_RUNTIME, external_signer_hook HOOK, and others). DispatchHostOp and Protocol were assumed identical and freely mergeable. A stale sibling PR (#1312/#619) addressed compile-time install-before-dispatch ordering.

## Trigger

K2 keystone plan to eliminate ambient authority; adversarial review of ADR-0052 found a 5th missed global (nmp-core/external_signer_hook.rs:38 HOOK) and revealed DispatchHostOp has panic-isolation + persistent-handler semantics that Protocol lacks — silently merging them would weaken Marmot handler safety.

## Decision

ADR-0052: convert ActionModule trait to &self + register-by-value; delete all five ambient-authority globals; merge DispatchHostOp into Protocol only after reproducing its panic-isolation guarantee (with a behavioural oracle); WalletRuntimeHandle already value-carried by WalletInterceptor from the composition root, so ACTIVE_WALLET_RUNTIME deletion is safe — extend the same value-threading to all action modules; #1312/#619 superseded by construction (value-registration makes install-before-dispatch automatic).

## Consequences

- No ambient authority for wallet/signer state — all access is value-threaded from composition root
- DispatchHostOp's panic-isolation guarantee must be preserved when merging into Protocol (behavioural oracle required in rung 5.4)
- D21 (not D20, which #1311 holds) reserved for the no-ambient-authority doctrine lint as regression gate
- d12 doctrine lint and nmp-cli templates must migrate in the same PR as the trait change
- Rung 5.2 blast radius: ~30 ActionModule impls across ~10 crates plus all register_action call sites and registrar traits
- #1312/#619 closes with rung 5.2 by construction — no race needed

## Open Tail

- Rungs 5.2–5.6 implementation in progress; 5.1 (ADR) merged, 5.2 implementer dispatched

## Evidence

- transcript lines 5554-5584
- transcript lines 5625-5648
- transcript lines 5692-5718

