---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - kernel-mut
  - active-wallet-runtime
  - global-broker
  - global-driver
  - hook-statics
  - d21-ambient-authority-lint
  - wallet-kernel-access
  - zap-profile-lookup
  - host-op-handler-access
supersedes:
  - 2026-06-13-2-instance-scoped-registration-replaces-ambient-authority
related_claims: []
source_lines:
  - 5712-5720
  - 5812-5821
  - 5860-5873
  - 5896-5910
  - 5947-5954
captured_at: 2026-06-14T11:30:42Z
---

# Episode: Ambient authority eliminated: five globals and kernel_mut replaced by narrow capability traits with D21 regression gate

## Prior State

Five process-global mutable singletons (ACTIVE_WALLET_RUNTIME, GLOBAL_BROKER, GLOBAL_DRIVER, + two hook statics) plus kernel_mut() escape hatch gave ProtocolCommands ambient &mut Kernel access. ActionModules registered by handle; DispatchHostOp was a separate actor. Two-instance interop was impossible because globals shared state across instances.

## Trigger

Excellence program P5 finding (ambient authority pattern); ADR-0052 instance-scoped extensibility design. The delegating-lead orchestration was also dropped mid-keystone because it was burning turns on CI waits and duplicate-ownership fights without advancing.

## Decision

All five globals deleted; kernel_mut() deleted. ActionModules now register by value (rung 5.2). Per-app bunker/NIP-55 signer ports (5.3). DispatchHostOp merged into Protocol arm with whole-body catch_unwind and narrow HostOpHandlerAccess capability instead of kernel_mut (5.4). kernel_mut replaced by WalletKernelAccess (exactly 9 kernel methods the wallet runtime needs) and ZapProfileLookup for the relocated lnurl_for_pubkey (5.5). D21 doctrine lint bans ambient-authority statics in production nmp-* crates (5.6).

## Consequences

- Two-instance interop proven; no ambient authority escape hatches remain
- D21 lint structurally prevents silent recurrence of process-global mutable singletons
- DispatchHostOp's panic-isolation and persistent-handler semantics preserved via HostOpHandlerAccess capability
- WalletKernelAccess adapter uses RefCell<&mut Kernel> with per-call try_borrow_mut, not a global
- File-size baselines retired rather than bumped (protocol.rs 650→538, action/tests.rs 977→721)
- Orchestration shifted from delegating-lead to direct-per-rung implementer after the lead layer proved wasteful

## Open Tail

*(none)*

## Evidence

- transcript lines 5712-5720
- transcript lines 5812-5821
- transcript lines 5860-5873
- transcript lines 5896-5910
- transcript lines 5947-5954

