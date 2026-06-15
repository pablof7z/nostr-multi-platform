---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - optimistic-local-echo
  - non-replaceable-events
  - ingest-admission-gate
supersedes:
  - 2026-06-15-4-issue-1440-research-suggested-fix-has
related_claims: []
source_lines:
  - 1-143
captured_at: 2026-06-15T17:04:02Z
---

# Episode: Issue #1440: admission gate + double-fire root causes block naive optimistic echo

## Prior State

Non-replaceable events (kind:1/6/7) were invisible locally after publish — only surfaced on relay echo. The issue's suggested fix called both verify_and_persist and ingest_timeline_event, then notify_event_observers again.

## Trigger

Researching the actual ingest pipeline revealed two critical flaws in the suggested fix and one hidden gate the issue missed entirely.

## Decision

Implement a new record_local_timeline_intent arm split by which relay arm the kind uses (timeline kinds vs. wildcard), with direct store+timeline-read-cache append for self-authored events that bypass the should_store_event admission gate (which would reject them since the user is normally not in their own follow set).

## Consequences

- Double store-insert avoided: must NOT call both verify_and_persist and ingest_timeline_event for the same event
- Double observer fire avoided: ingest_timeline_event already fires notify_event_observers at timeline.rs:252; calling it again violates the D4 single-fire invariant
- Admission gate is the crux: should_store_event requires timeline_authors.contains(author); a self-authored note is silently dropped unless open_interest matches; naive reuse of ingest_timeline_event would fail for fresh root notes
- verify_and_persist alone misses the timeline read-cache append, so the feed snapshot still wouldn't show the event

## Open Tail

- Implementation of record_local_timeline_intent not yet landed in this session; depends on PR 2/3 infrastructure

## Evidence

- transcript lines 1-143
