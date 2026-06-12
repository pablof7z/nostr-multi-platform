---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: reversal
status: active
subjects:
  - zap-spec
  - v1-scope
  - m12-wallet
supersedes: []
related_claims: []
source_lines:
  - 2950-2965
captured_at: 2026-06-11T23:31:21Z
---

# Episode: Zap/wallet moved from v1 to post-v1

## Prior State

Four contradictory statements across plan.md, post-v1.md, and m12-wallet.md listed zap receipt nostrPubkey verification, nutzap claim, and Cashu as v1 requirements; issue #978 was closed but the spec still implied they might ship pre-v1.

## Trigger

Owner decision to definitively move zap to post-v1, resolving contradictions across planning docs.

## Decision

All zap features sectioned out to post-v1; plan.md, post-v1.md, and m12-wallet.md reconciled to agree; issues #610/#1022/#1043 relabeled; #978 definitively closed.

## Consequences

- Single source of truth restored — doctrine lint 46/46 passes
- v1 scope is now smaller and honest
- post-v1.md carries the definitive owner-decision statement

## Open Tail

- Zap implementation still needs post-v1 scheduling

## Evidence

- transcript lines 2950-2965

