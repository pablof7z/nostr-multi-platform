---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - optimistic-local-echo
  - should-store-event-gate
  - issue-1440
supersedes: []
related_claims: []
source_lines:
  - 124-147
captured_at: 2026-06-15T15:14:58Z
---

# Episode: Admission gate blocks naive optimistic echo for non-replaceable events

## Prior State

Issue #1440 suggested calling verify_and_persist then ingest_timeline_event for local echo of non-replaceable events (kind:1/6/7); it was assumed that reusing the existing relay ingest path would work

## Trigger

Research into should_store_event (timeline.rs:299) revealed its primary clause is timeline_authors.contains(author) — a user is normally not in their own follow set, so a self-authored note would be dropped or parked into pre_kind3_buffer; additionally the issue's suggested fix causes double store-insert, double dispatcher fan-out, and double observer fire (violating the D4 single-fire invariant)

## Decision

Cannot naively reuse ingest_timeline_event for optimistic local echo; need a new arm record_local_timeline_intent that (1) ensures self-authored events pass the admission gate without requiring the author to be in their own follow set, and (2) appends to the timeline read-cache (which verify_and_persist alone does not do); the two relay arms must be mirrored separately — timeline kinds use the timeline path, other non-replaceables use verify_and_persist + observer notify

## Consequences

- The issue's suggested fix would silently fail for fresh root notes (self-authored notes dropped by the admission gate)
- verify_and_persist alone is insufficient — it never appends to the timeline read-cache, so the feed snapshot still wouldn't show the event
- The fix must preserve D4 single-fire invariant: timeline path fires observers internally (timeline.rs:252), the wildcard path requires a separate notify_event_observers call — never both

## Open Tail

- Implementation of record_local_timeline_intent deferred; depends on the unified chokepoint (PR 1) and capability seams (PR 2/3) landing first

## Evidence

- transcript lines 124-147
