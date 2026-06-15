---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - workstream-b-remaining
  - interest-registry-vs-outbox-router
  - profile-claim-direct-req
supersedes: []
related_claims: []
source_lines:
  - 3447-3523
captured_at: 2026-06-15T16:55:55Z
---

# Episode: Workstream B audit: #1436 was outbox-router, not InterestRegistry — all 4 items remain

## Prior State

Workstream B (acquisition one-door) was believed largely done via commit 22f3832a7 (#1436), which was interpreted as having migrated profile-claim and reverify onto the InterestRegistry/LogicalInterest model.

## Trigger

B/D/E residual audit inspected actual code: profile_claim_request still builds direct REQs via self.req_for_relay (profile.rs:278,315); drain_pending_reverify builds direct REQs via self.req_for_relay (mod.rs:395); probed_mailboxes is insert-only with no TTL/epoch; no doctrine-lint rule bans req_for_relay outside compiler/lifecycle.

## Decision

All 4 B items remain genuinely open. The 'registry chokepoint' wording in commit 22f3832a7 referred to the outbox-router/10002-discovery path, not the InterestRegistry/LogicalInterest model the B-plan items specify. B status corrected in program memory.

## Consequences

- B1: profile claim/release must still be converted to owner-keyed LogicalInterests (kill profile_requests pipeline + req_for_relay calls)
- B2: reverify must be modeled as registry-owned freshness/one-shot keyed by ReplaceableKey (kill direct REQ build)
- B3: mailbox-discovery epoch/probe lifecycle needed (probed_mailboxes is permanent-per-session today)
- B4: one-door lint banning req_for_relay outside compiler/lifecycle (overlaps with Workstream F)
- Most B items conflict with PR2's in-flight edits, so they queue behind it

## Open Tail

- B1/B2 implementation after PR3 lands
- B3 epoch/probe design choice: TTL vs generation vs indexer-reconnect signal

## Evidence

- transcript lines 3447-3523
