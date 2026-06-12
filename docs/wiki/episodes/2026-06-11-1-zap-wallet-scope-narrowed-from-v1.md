---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: reversal
status: active
subjects:
  - zap-scope
  - m12-wallet
  - v1-exit-criteria
supersedes: []
related_claims: []
source_lines:
  - 2937-2968
captured_at: 2026-06-11T23:22:45Z
---

# Episode: Zap/wallet scope narrowed from v1 to post-v1

## Prior State

Zap features (receipt nostrPubkey extraction, ZapRequestBuilder sentinel API, zap_subscription typed-sidecar) were listed as v1 blockers; M12 milestone row said 'F-04 E2E pending'; m12-wallet.md exit gate included receipt verification, nutzap claim, and Cashu as v1 requirements.

## Trigger

Owner decision to defer zap work past v1, made explicit during the session.

## Decision

Relabeled all three zap issues (#1043, #610, #1022) as phase:post-v1; updated M12 milestone to '✅ shipped; further zap work post-v1 by owner decision'; sectioned unbuilt v1 requirements out of m12-wallet.md into post-v1; removed closed F-04 from v1-blockers list.

## Consequences

- v1 exit criteria no longer depend on any zap/wallet deliverable
- post-v1.md carries the definitive owner-decision statement replacing ambiguous language
- four contradictory statements across plan.md, post-v1.md, and m12-wallet.md were resolved

## Open Tail

- Stages 1-2 of ADR-0045 (store replay) still gate v1; zap is orthogonal

## Evidence

- transcript lines 2937-2968

