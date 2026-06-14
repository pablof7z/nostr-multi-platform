---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: active
subjects:
  - ambient-authority-elimination
  - kernel-mut-deletion
  - instance-scoped-registration
supersedes:
  - 2026-06-14-1-k2-ambient-authority-eliminated-narrow-capability
  - 2026-06-13-2-instance-scoped-registration-replaces-ambient-authority
related_claims: []
source_lines:
  - 5860-5967
captured_at: 2026-06-14T13:26:23Z
---

# Episode: Ambient authority → instance-scoped capability model (K2)

## Prior State

Five process-global singletons (ACTIVE_WALLET_RUNTIME, GLOBAL_BROKER, GLOBAL_DRIVER, two hook statics) and ProtocolCommandContext::kernel_mut() provided ambient authority — any command could reach the entire &mut Kernel or shared global state, creating coupling and preventing instance isolation.

## Trigger

Excellence program finding P5: ambient authority pattern allows any ProtocolCommand to access full kernel, creating hidden coupling and making two-instance interop impossible; no structural gate prevents recurrence.

## Decision

Delete all five globals and kernel_mut(). Replace with instance-scoped registration (register ActionModules by value, per-app bunker + NIP-55 signer ports) and narrow capability traits: WalletKernelAccess (exactly the 9 kernel methods the NIP-47 wallet runtime mutates) and ZapProfileLookup (zap-only lnurl_for_pubkey read, moved off the generic context). Add D21 doctrine-lint as a permanent regression gate banning ambient-authority statics in production nmp-* crates.

## Consequences

- Two-instance interop proven — no shared globals block it
- Commands can only access exactly the kernel surface their capability trait permits
- D21 lint structurally prevents ambient-authority globals from silently returning
- All panic-isolation oracles preserved (whole-body catch_unwind on ProtocolCommand)
- ProtocolCommandContext no longer carries &mut Kernel; dispatch uses RefCell-borrowed WalletKernelAccessAdapter

## Open Tail

- D21 allowlist currently contains two benign read-once logging OnceLocks (wire_log, socket_io); future logging globals need explicit D21 exception

## Evidence

- transcript lines 5860-5967

