---
type: episode-card
date: 2026-05-26
session: 95156e27-58fe-4e26-9530-1778033c4559
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/95156e27-58fe-4e26-9530-1778033c4559.jsonl
salience: root-cause
status: active
subjects:
  - protocol-command-context
  - refcell-borrow-conflict
supersedes: []
related_claims: []
source_lines:
  - 1410-1416
  - 1688-1697
  - 1970-2016
  - 2055-2058
captured_at: 2026-06-18T05:50:28Z
---

# Episode: ProtocolCommandContext bypasses KernelClockAdapter RefCell to prevent double-borrow crash

## Prior State

ProtocolCommandContext::now_secs() delegated to KernelClockAdapter which called kernel_cell.borrow() inside cmd.run(), while the dispatch arm held kernel_cell.borrow_mut() for the entire cmd.run() scope; catch_unwind was assumed to absorb the resulting panic, but it did not prevent the crash

## Trigger

Zap from palette exercised FetchLnurlInvoiceCommand for the first time, causing 'RefCell already mutably borrowed' panic at dispatch.rs:317 — a latent bug exposed by the new execution path

## Decision

ProtocolCommandContext::now_secs() now uses self.kernel.as_deref() to call kernel.now_secs() directly when a kernel handle is attached (always true in production), bypassing KernelClockAdapter's RefCell entirely; falls back to the clock adapter + catch_unwind only in test contexts where kernel is None

## Consequences

- All ProtocolCommand implementations can safely call ctx.now_secs() during cmd.run() without RefCell conflict
- catch_unwind wrapper preserved as defense-in-depth for test-only adapter paths
- Same latent RefCell conflict pattern may exist for other KernelClockAdapter methods (signers, etc.) but is not triggered by current command paths

## Open Tail

- Other KernelClockAdapter/LocaleSignerAccessAdapter RefCell accessors could hit the same borrow-conflict if future ProtocolCommands exercise them during cmd.run()

## Evidence

- transcript lines 1410-1416
- transcript lines 1688-1697
- transcript lines 1970-2016
- transcript lines 2055-2058

