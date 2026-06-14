---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - ambient-authority-removal
  - kernel-mut-deletion
  - capability-traits
  - d21-lint
supersedes:
  - 2026-06-14-1-ambient-authority-eliminated-five-globals-and
related_claims: []
source_lines:
  - 5746-5967
captured_at: 2026-06-14T12:30:14Z
---

# Episode: K2: Ambient authority eliminated — narrow capability traits replace globals + kernel_mut

## Prior State

Five process-global singletons (GLOBAL_BROKER, GLOBAL_DRIVER, ACTIVE_WALLET_RUNTIME, and both hook statics) and ProtocolCommandContext::kernel_mut() provided ambient authority escape hatches — any code path could grab the entire &mut Kernel or a global broker without scoped capability.

## Trigger

Excellence program P5 (ambient authority) and ADR-0052 required eliminating all ambient authority access patterns to the kernel.

## Decision

Delete all 5 globals + kernel_mut(); replace with narrow capability traits — WalletKernelAccess (exactly 9 methods the wallet runtime needs), ZapProfileLookup (isolated lnurl_for_pubkey), HostOpHandlerAccess (preserves panic isolation); add D21 doctrine lint banning ambient-authority statics in nmp-* production code as a regression gate.

## Consequences

- Per-app bunker and NIP-55 signer ports replace global broker/driver (rung 5.3)
- Wallet runtime reaches only 9 kernel methods via WalletKernelAccess, not the entire &mut Kernel
- Zap profile lookup relocated off generic context onto dedicated ZapProfileLookup capability
- DispatchHostOp merged into Protocol seam with HostOpHandlerAccess for panic isolation (rung 5.4)
- D21 lint structurally prevents recurrence of ambient-authority globals
- File-size baselines lowered (protocol.rs 650→538, action/tests.rs 977→721) — debt retired, not bumped

## Open Tail

*(none)*

## Evidence

- transcript lines 5746-5967

