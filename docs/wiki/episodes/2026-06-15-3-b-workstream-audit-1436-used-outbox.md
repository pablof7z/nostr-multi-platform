---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - workstream-b-status
  - interest-registry-vs-outbox-router
  - req-for-relay-one-door
supersedes:
  - 2026-06-15-5-workstream-b-status-reversal-1436-did
related_claims: []
source_lines:
  - 3447-3522
captured_at: 2026-06-15T15:36:07Z
---

# Episode: B workstream audit: #1436 used outbox router, not InterestRegistry — all 4 items remain open

## Prior State

Workstream B (acquisition one-door) was believed to be largely done — #1436 / commit 22f3832a7 was understood to have migrated profile-claim and reverify onto the InterestRegistry/LogicalInterest model.

## Trigger

Read-only audit of master @ 1e03b94ce examined actual code: profile_claim_request still builds direct REQs via req_for_relay (profile.rs:278,315), drain_pending_reverify builds direct REQs via req_for_relay (mod.rs:395), probed_mailboxes has no epoch/TTL, and no doctrine lint bans req_for_relay outside compiler/lifecycle.

## Decision

Correct the program ledger: #1436's 'registry chokepoint' referred to the outbox-router/10002-discovery path, NOT the InterestRegistry/LogicalInterest model the B-plan specifies. All 4 B items genuinely remain open.

## Consequences

- B1: profile claim/release must still be converted to owner-keyed LogicalInterests — profile_requests pipeline and req_for_relay calls must be deleted
- B2: reverify must still be modeled as registry-owned freshness/one-shot keyed by ReplaceableKey
- B3: mailbox discovery probe set needs epoch/TTL/generation so empty-EOSE-probed authors are re-probed after outage
- B4: one-door lint banning req_for_relay outside compiler/lifecycle still absent (will also be closed by Workstream F)
- Most B items conflict with PR 2's in-flight edits and queue behind it

## Open Tail

- B1/B2 require InterestRegistry/LogicalInterest migration; B3 needs epoch/probe lifecycle design; B4 blocked on Workstream F doctrine gates

## Evidence

- transcript lines 3447-3522
