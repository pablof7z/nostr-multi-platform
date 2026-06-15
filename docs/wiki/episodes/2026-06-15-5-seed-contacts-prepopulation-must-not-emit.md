---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: product
status: active
subjects:
  - seed-contacts-phantom-event
  - contacts-prepopulate
  - observer-fan-out-contract
supersedes: []
related_claims: []
source_lines:
  - 4332-4408
captured_at: 2026-06-15T18:08:06Z
---

# Episode: Seed contacts prepopulation must not emit phantom events to observers

## Prior State

prepopulate_seed_contacts (sign-in path) fabricated a synthetic kind:3 event with fake id/sig and pushed it through project_accepted_event, which unconditionally notifies KernelEventObservers — emitting a non-persisted fake kind:3 to the app, then the real signed kind:3 later (double-fire + phantom event).

## Trigger

Codex review of PR 3 caught that project_accepted_event's contract is accepted post-store fan-out only and unconditionally notifies observers — the exact 'no fake event through the fan-out' lesson from PR 2, recurring in the seed path.

## Decision

Prepopulate now uses ContactsLookup::upsert (a non-ingest writer) + on_active_contacts_changed WITHOUT observer fan-out, mirroring MailboxCache's sign-in seed pattern. No fake event is constructed; no phantom event reaches app observers.

## Consequences

- Sign-in no longer emits phantom kind:3 events to KernelEventObservers
- The kernel-internal seed path writes cache + drives effects directly without the observer fan-out contract
- Pattern established: seed/prepopulate paths must use dedicated non-observer writers, never project_accepted_event with synthetic events

## Open Tail

*(none)*

## Evidence

- transcript lines 4332-4408
