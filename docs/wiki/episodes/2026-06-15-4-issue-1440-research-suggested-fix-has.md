---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - optimistic-local-echo
  - non-replaceable-read-your-writes
  - ingest-timeline-admission-gate
supersedes:
  - 2026-06-15-1-non-replaceable-optimistic-local-echo-requires
related_claims: []
source_lines:
  - 124-148
captured_at: 2026-06-15T16:55:55Z
---

# Episode: Issue #1440 research: suggested fix has double-fire + admission-gate flaws

## Prior State

Issue #1440 proposed that non-replaceable optimistic echo could be implemented by calling both verify_and_persist and ingest_timeline_event + notify_event_observers for locally-published kind:1/6/7 events.

## Trigger

Code research of the actual ingest pipeline revealed two flaws in the suggested approach plus a critical admission gate problem.

## Decision

The suggested fix is incorrect. Two relay ingest arms exist (kind:1/6 → ingest_timeline_event which is self-contained including observer fire; kind:7 → verify_and_persist + separate notify_event_observers). Calling both paths causes double store-insert + double observer fire (violating D4 single-fire). The real crux: should_store_event's primary clause is timeline_authors.contains(author), and a user is normally NOT in their own follow set, so a self-authored root note would be silently dropped or parked. Plan: add a new record_local_timeline_intent arm that mirrors each relay arm exactly, with an admission gate bypass for self-authored events.

## Consequences

- Naive reuse of ingest_timeline_event for local echo would silently fail for fresh root notes (user not in own follow set)
- verify_and_persist alone doesn't append to the timeline read-cache, so the feed snapshot still wouldn't show the event
- Implementation must split: timeline kinds (1/6) need the timeline read-cache append path with admission gate bypass; non-timeline non-replaceable kinds (7+) need verify_and_persist + notify_event_observers
- Both paths must preserve D4 single-fire invariant — no double dispatch or double observer fire

## Open Tail

- record_local_timeline_intent implementation not yet started (queued behind current PR wave)

## Evidence

- transcript lines 124-148
