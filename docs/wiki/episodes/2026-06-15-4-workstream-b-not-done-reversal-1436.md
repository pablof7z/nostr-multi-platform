---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: reversal
status: active
subjects:
  - workstream-b-acquisition
  - interest-registry
  - mailbox-probe-lifecycle
supersedes:
  - 2026-06-15-5-workstream-b-audit-not-done-1436
  - 2026-06-15-2-workstream-b-audit-1436-was-outbox
related_claims: []
source_lines:
  - 3447-3531
captured_at: 2026-06-15T17:04:02Z
---

# Episode: Workstream B NOT-DONE reversal: #1436 migrated to outbox router, not InterestRegistry

## Prior State

Prior belief (per commit 22f3832a7's 'registry chokepoint' wording) was that Workstream B was largely done via #1436 — profile-claim and reverify had been migrated onto the registry.

## Trigger

Read-only audit of actual code: claim_profile still drives profile_requests pending/requested sets and builds direct REQs via req_for_relay (profile.rs:278,315); drain_pending_reverify builds direct REQs via req_for_relay (mod.rs:395); probed_mailboxes is insert-only per session with no TTL/epoch/generation.

## Decision

Workstream B is NOT done — all four items remain. #1436 migrated profile-claim/reverify onto the outbox router (route_outbox_subscription_relays), not the InterestRegistry/LogicalInterest model. The 'registry chokepoint' commit wording referred to the outbox-router/10002-discovery path, not the InterestRegistry. B3 (mailbox discovery epoch/probe lifecycle) has no re-arm mechanism: probed_mailboxes is permanent per session, only cleared on config change; the reconnect re-arm was explicitly reverted due to web-feed regression.

## Consequences

- B1: profile claim/release must still be converted to owner-keyed LogicalInterests; profile_requests pipeline + req_for_relay calls must be deleted
- B2: reverify must be modeled as registry one-shot/freshness keyed by ReplaceableKey; direct REQ construction must be deleted
- B3: probed_mailboxes needs epoch/probe lifecycle (TTL or generation) so empty-EOSE-probed authors are re-probed on indexer reconnect without churning the live sub
- B4: one-door doctrine lint banning req_for_relay outside compiler/lifecycle does not exist yet (none of D6–D21 rules cover it)
- Memory ledger corrected: B status updated from 'done' to 'genuinely open'

## Open Tail

- B1/B2 conflict with PR 2's in-flight chokepoint edits — queued behind PR 2 landing
- B3 reconnect re-arm was previously reverted (#1436 follow-up) due to web-feed regression — needs a different approach

## Evidence

- transcript lines 3447-3531
