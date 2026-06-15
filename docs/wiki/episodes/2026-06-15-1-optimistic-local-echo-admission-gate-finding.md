---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: active
subjects:
  - local-publish-intent
  - timeline-admission-gate
  - non-replaceable-echo
supersedes:
  - 2026-06-15-1-issue-1440-admission-gate-double-fire
related_claims: []
source_lines:
  - 1-148
captured_at: 2026-06-15T17:37:54Z
---

# Episode: Optimistic local echo admission-gate finding (issue #1440)

## Prior State

Issue #1440 suggested calling both verify_and_persist and ingest_timeline_event for non-replaceable local publishes, assuming the relay ingest path could be reused directly for optimistic echo.

## Trigger

Code research revealed (1) calling both verify_and_persist AND ingest_timeline_event causes double store-insert and double observer fire (violating D4 single-fire invariant), and (2) the real crux: should_store_event's primary clause is timeline_authors.contains(author) — a user is normally NOT in their own follow set, so a self-authored root note would be silently dropped by the admission gate or parked into pre_kind3_buffer.

## Decision

New arm record_local_timeline_intent must be added to record_local_publish_intent, split by which relay arm the kind uses (timeline kinds vs wildcard), bypassing the should_store_event admission gate for self-authored events while preserving single-insert and single-observer-fire semantics.

## Consequences

- The naive reuse of ingest_timeline_event would silently fail for fresh root notes — the admission gate rejects authors not in timeline_authors
- verify_and_persist alone misses the timeline read-cache append, so the feed snapshot still wouldn't show the note
- Two distinct relay arms must be mirrored: timeline kinds (kind:1/6) use ingest_timeline_event internally; wildcard kinds (kind:7+) use verify_and_persist + separate notify_event_observers
- Any fix must not violate D4 single-fire — no double insert or double observer notification

## Open Tail

- Implementation of record_local_timeline_intent not yet in this session — plan formed but not landed
- Handling of pre_kind3_buffer interaction for edge cases where self-authored note arrives before follow set is processed

## Evidence

- transcript lines 1-148
