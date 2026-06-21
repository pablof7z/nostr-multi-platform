---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - cache-serve
  - store-wakeups
  - actor-gate
supersedes: []
related_claims: []
source_lines:
  - 3303-3311
  - 3423-3443
  - 3865-3876
captured_at: 2026-06-18T20:04:31Z
---

# Episode: Event-driven cache-serve wakeups replace polling

## Prior State

Cache-serve projections relied on polling loops or broad recompute scans to detect new data matching their interests. No mechanism existed to wake already-served projections when matching events arrived live.

## Trigger

Epic #1523 sub-issue #1520: adapt nostrdb's subscription wakeup shape for NMP cache projections, routing through the existing Rust-owned composite reverse index without standing up a parallel index.

## Decision

Store-insert notifications fire from the live-ingest path (accepted.rs after project_accepted_event), walking active (SubKey, LogicalInterest) pairs via the existing registry. If the interest is already fully served (its completion_key is in served_interest_shapes), the key is inserted into a coalescing BTreeSet<u64>. drain_cache_serve_wakeups empties the set on each actor tick, re-enqueues matched live interests, and silently drops stale keys for closed views. Actor gate extended: has_pending_cache_serves() || has_cache_serve_wakeups().

## Consequences

- No polling loops, no timers, no unbounded channels — D8 compliant
- Pending interests are NOT double-served: note_store_insert only inserts into cache_serve_wakeups when the interest is already in served_interest_shapes; register-time serve handles pending interests
- Multiple inserts between actor ticks coalesce into a single wakeup per interest (BTreeSet deduplication)
- Closed views are silently dropped during drain — no stale references

## Open Tail

*(none)*

## Evidence

- transcript lines 3303-3311
- transcript lines 3423-3443
- transcript lines 3865-3876

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-event-driven-cache-serve-wakeups-replace.json`](transcripts/2026-06-18-3-event-driven-cache-serve-wakeups-replace.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-event-driven-cache-serve-wakeups-replace.json`](transcripts/raw/2026-06-18-3-event-driven-cache-serve-wakeups-replace.json)
