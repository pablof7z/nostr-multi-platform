---
type: episode-card
date: 2026-06-03
session: f1b740a8-d601-4b63-8633-072c83a6de22
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f1b740a8-d601-4b63-8633-072c83a6de22.jsonl
salience: product
status: active
subjects:
  - nip-57-zap
  - nip-46-bunker
  - local-signer-access
  - protocol-command
supersedes: []
related_claims: []
source_lines:
  - 3479-3490
  - 3619-3653
  - 3780-3807
  - 3686-3710
captured_at: 2026-06-11T23:05:43Z
---

# Episode: V-78: bunker accounts can now zap via nonblocking sign seam

## Prior State

NIP-46 bunker accounts could not zap; the kind:9734 zap request required `active_local_keys()` which returns `None` for bunker accounts, producing a dead-end toast and silent failure

## Trigger

V-78 backlog item; code inspection confirmed `lnurl/mod.rs:200` gated on `active_local_keys()` returning `Some(Keys)` — a D13 anti-pattern

## Decision

Added `sign_active_nonblocking` to `LocalSignerAccess` trait and `ProtocolCommandContext`; zap command resolves `SignerOp` on-actor (non-blocking), then calls `op.wait(10s)` off-actor on the existing HTTP worker thread — mirroring the DM-send pattern (ADR-0040 Site 1). Local keys sign instantly; bunker accounts resolve asynchronously via NIP-46.

## Consequences

- All `LocalSignerAccess` impls must now implement `sign_active_nonblocking` (CI caught two test stubs missing it)
- Bunker accounts can now zap — user-visible feature fix
- Async-sign idiom standardized: 'resolve SignerOp on-actor, op.wait() off-actor' — NOT PendingSign park (which is only reachable from non-Protocol dispatch arms)
- `signed_event_to_nostr_json` added to rebuild flat NIP-01 wire format from the nested `SignedEvent` struct, proven byte-identical to old local path

## Open Tail

- PR #938 merged; no known follow-ups for the zap path itself
- Other ProtocolCommand paths that currently gate on `active_local_keys()` may need the same migration

## Evidence

- transcript lines 3479-3490
- transcript lines 3619-3653
- transcript lines 3780-3807
- transcript lines 3686-3710

