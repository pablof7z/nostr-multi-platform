---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: product
status: superseded
subjects:
  - local-echo-non-replaceable
  - ingest-timeline-admission-gate
supersedes:
  - 2026-06-15-3-non-replaceable-optimistic-echo-admission-gate
related_claims: []
source_lines:
  - 1-148
captured_at: 2026-06-15T16:41:53Z
---

# Episode: Non-replaceable optimistic local echo requires admission-gate bypass

## Prior State

Only replaceable events (kind:0/3/10002) received read-your-writes echo via record_local_publish_intent. Non-replaceables (kind:1/6/7) were invisible until relay round-trip — the 'ghost post' UX gap in issue #1440.

## Trigger

Issue #1440 investigation + code research revealed two critical flaws in the issue's suggested fix: (1) calling both verify_and_persist and ingest_timeline_event causes double store-insert and double dispatcher fan-out; (2) ingest_timeline_event's should_store_event admission gate (timeline_authors.contains) will drop self-authored notes because a user is normally not in their own follow set — naive reuse silently fails for fresh root notes.

## Decision

Add a new record_local_timeline_intent arm split by which relay arm the kind uses: timeline kinds (kind:1/6) route through ingest_timeline_event with the admission gate bypassed (self-authored notes must pass), other non-replaceables (kind:7+) route through verify_and_persist + notify_event_observers. Timeline read-cache append is mandatory for feed visibility. The double-fire D4 invariant must be preserved exactly.

## Consequences

- Admission gate is the single most important constraint — force-persisting via verify_and_persist alone makes the event invisible to the timeline read-cache
- Two distinct relay arms must be mirrored exactly: timeline kinds are self-contained (store+dispatch+notify internally), kind:7+ requires separate notify after verify_and_persist
- pre_kind3_buffer parking risk if admission gate is not bypassed
- Issue's suggested fix would violate D4 single-fire invariant

## Open Tail

- Implementation of the new arm not yet started in this session
- Exact mechanism for bypassing should_store_event for self-authored events needs design

## Evidence

- transcript lines 1-148
