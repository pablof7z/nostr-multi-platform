---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: reversal
status: superseded
subjects:
  - workstream-b-status
  - interest-registry
  - logical-interest
  - outbox-router-vs-registry
supersedes: []
related_claims: []
source_lines:
  - 3447-3522
captured_at: 2026-06-15T15:26:03Z
---

# Episode: Workstream B status reversal — #1436 did not complete InterestRegistry migration

## Prior State

Workstream B was believed to be largely done via commit #1436 (`22f3832a7`), which described itself as a 'registry chokepoint' migration for profile claims and reverify.

## Trigger

Read-only residual audit of B/D/E found that #1436's 'registry chokepoint' refers to the outbox router (`route_outbox_subscription_relays`), NOT the `InterestRegistry`/`LogicalInterest` model the B-plan specifies. Profile claims and reverify still build direct `req_for_relay` REQs with hand-built JSON filters.

## Decision

All 4 B items genuinely remain open: B1 (profile claim → owner-keyed LogicalInterest), B2 (reverify → registry-owned freshness/one-shot), B3 (mailbox-discovery epoch/probe lifecycle — probed_mailboxes is insert-only per session with no TTL/epoch), B4 (one-door lint banning req_for_relay outside compiler/lifecycle). Previous 'done' assumption is reversed.

## Consequences

- B1/B2 still construct direct REQs via `req_for_relay` (profile.rs:278,315 and mod.rs:395) — must be converted to LogicalInterest registrations
- B3: probed_mailboxes is permanent per-session; only re-arm is config-change clear_probed_mailboxes(); reconnect re-arm was explicitly reverted in #1436 follow-up due to web-feed regression
- B4: no doctrine-lint rule references req_for_relay (D6–D21 exist; none cover this); Workstream F gates (store.insert, notify_event_observers, D22) also absent
- B items conflict with PR 2 in-flight edits and must wait for it to land

## Open Tail

- B3 mailbox epoch/probe lifecycle is the most architecturally subtle item — needs design for re-probing after indexer outage without churning live subscriptions
- B4 lint can be bundled with Workstream F doctrine gates

## Evidence

- transcript lines 3447-3522
