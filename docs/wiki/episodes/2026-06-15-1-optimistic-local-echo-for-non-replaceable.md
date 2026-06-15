---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - local-publish-intent
  - non-replaceable-echo
  - ingest-timeline-admission-gate
supersedes: []
related_claims: []
source_lines:
  - 124-156
captured_at: 2026-06-15T08:17:13Z
---

# Episode: Optimistic local echo for non-replaceable events — architecture and admission-gate fix

## Prior State

record_local_publish_intent only echoed replaceable events (kind:0/3/10002); non-replaceable events (kind:1 notes/replies, kind:6 reposts, kind:7 reactions) were invisible locally until relay round-trip confirmation. The issue's suggested fix called both verify_and_persist and ingest_timeline_event plus manual observer notification, producing double inserts and double observer fires.

## Trigger

Issue #1440 (ghost-post UX) plus code-level investigation revealing two flaws in the proposed fix and a critical admission-gate gap: should_store_event gates on timeline_authors.contains(author), and a user is normally not in their own follow set — so self-authored fresh root notes would be silently dropped or parked in pre_kind3_buffer under a naive implementation.

## Decision

Add record_local_timeline_intent arm to record_local_publish_intent, split by relay routing: (1) timeline kinds (kind:1/6) route through ingest_timeline_event with a self-publish admission clause (sentinel sub_id "local://publish" mirroring the diag-firehose escape hatch) so the timeline read-cache, dispatcher, and observers all update via the single existing mechanism per ADR-0045 — no manual re-fire of observers; (2) other non-replaceable kinds (kind:7 etc.) mirror the wildcard arm exactly (verify_and_persist then notify_event_observers on Inserted), excluding gift-wraps (kind:1059); (3) gate by behavioral predicates (follow_feed_kinds.contains, is_replaceable, KIND_GIFT_WRAP) per D0 doctrine — no hardcoded NIP kind numbers.

## Consequences

- Users see their own non-replaceable posts immediately without relay round-trip (read-your-writes for all event kinds)
- D4 single-fire observer invariant preserved — no double dispatch or double observer notification
- Self-authored events unconditionally admitted via local://publish sentinel, bypassing follow-set membership check
- Relay echo of locally-echoed events dedups to Duplicate without re-firing observers
- Gift-wraps (kind:1059) explicitly excluded from local echo

## Open Tail

- Scope confirmation pending: implement all non-replaceables (kind:1/6/7) in one pass or narrow to kind:1 notes/replies first
- Test suite to be extended: local kind:1 publish timeline visibility + single observer fire; admission-gate regression guard (kind:1 with no active interest); relay-echo dedup no double-fire; kind:7 reaction echo; gift-wrap exclusion

## Evidence

- transcript lines 124-156
