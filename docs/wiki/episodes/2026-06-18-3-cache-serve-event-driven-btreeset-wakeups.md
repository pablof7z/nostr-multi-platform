---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: active
subjects:
  - cache-serve-wakeups
  - kernel-actor
  - ingest-accepted
supersedes:
  - 2026-06-18-3-event-driven-cache-serve-wakeups-replace
related_claims: []
source_lines:
  - 3423-3525
captured_at: 2026-06-18T20:17:13Z
---

# Episode: Cache-serve: event-driven BTreeSet wakeups replace no-notification model

## Prior State

Already-served cache projections had no mechanism to be re-armed when matching events arrived live; stale served interests would remain until next full recompute

## Trigger

Epic #1523 sub-issue #1520 — adopt nostrdb's subscription wakeup shape for NMP cache projections, without polling loops or unbounded channels

## Decision

BTreeSet<u64> cache_serve_wakeups coalescing buffer on Kernel; note_store_insert fires from live-ingest chokepoint (accepted.rs) after project_accepted_event; drain_cache_serve_wakeups runs as first statement in run_cache_serve_step, re-enqueuing matched interests; stale keys for closed views silently dropped; pending interests left alone (register-time serve handles them)

## Consequences

- D8-compliant: bounded (BTreeSet), no timers, no unbounded channels, coalesced per actor turn
- Actor gate extended: has_pending_cache_serves() || has_cache_serve_wakeups()
- No double-serving: only already-served interests (in served_interest_shapes) get wakeup entries; pending interests are handled at register time
- 5 new tests covering wakeup-on-insert, insert-before-registration, closed-view drop, coalesce-many-to-one, replay-chunk-no-duplicate

## Open Tail

*(none)*

## Evidence

- transcript lines 3423-3525

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-cache-serve-event-driven-btreeset-wakeups.json`](transcripts/2026-06-18-3-cache-serve-event-driven-btreeset-wakeups.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-cache-serve-event-driven-btreeset-wakeups.json`](transcripts/raw/2026-06-18-3-cache-serve-event-driven-btreeset-wakeups.json)
