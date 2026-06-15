---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - workstream-b-status
  - acquisition-one-door
  - mailbox-probe-epoch
supersedes:
  - 2026-06-15-3-b-workstream-audit-1436-used-outbox
related_claims: []
source_lines:
  - 3447-3530
captured_at: 2026-06-15T16:41:53Z
---

# Episode: Workstream B audit: not done — #1436 used outbox router, not InterestRegistry

## Prior State

Believed Workstream B was largely done via #1436 (commit 22f3832a7 'registry chokepoint'). Profile-claim and reverify were assumed migrated to InterestRegistry/LogicalInterest model. Mailbox probe lifecycle was assumed handled.

## Trigger

Read-only audit of actual code revealed #1436 only migrated profile-claim/reverify to the outbox router (route_outbox_subscription_relays), not to InterestRegistry/LogicalInterest. Both claim_profile and drain_pending_reverify still build direct REQs via req_for_relay with hand-built json! filters. probed_mailboxes is insert-only for the session with no TTL/epoch/generation — empty-EOSE-probed authors are permanently suppressed with no recovery path except config change.

## Decision

Correct the program ledger: all 4 B items remain open. B1/B2 require killing req_for_relay and converting to LogicalInterest/registry-owned interests. B3 requires epoch/probe lifecycle so empty-EOSE-probed authors are re-probed on indexer reconnect/outage recovery without churning live subs. B4 requires doctrine lint banning req_for_relay outside compiler/lifecycle.

## Consequences

- Significantly increases remaining scope — B was thought done, now fully open
- probed_mailboxes permanent suppression is a latent bug: indexer outage can permanently hide users
- B1/B2 conflict with PR 2's in-flight chokepoint edits — must queue behind it
- B4 (req_for_relay lint) overlaps with Workstream F gates
- E4 (empty declared-set permits all) contradicts ADR-0053 Decision 4 — flagged for owner decision, not auto-amended

## Open Tail

- B1: profile claim/release → owner-keyed LogicalInterest (kill profile_requests + req_for_relay)
- B2: reverify → registry-owned freshness/one-shot keyed by ReplaceableKey
- B3: mailbox epoch/probe lifecycle design needed
- E4: owner decision needed on empty declared-set warn vs ADR-0053 permit-all

## Evidence

- transcript lines 3447-3530
