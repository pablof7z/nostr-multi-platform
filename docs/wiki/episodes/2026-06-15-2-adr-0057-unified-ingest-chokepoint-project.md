---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: active
subjects:
  - nmp-core-ingest
  - adr-0057
  - project-accepted-event
supersedes:
  - 2026-06-15-2-unified-post-store-fan-out-via
related_claims: []
source_lines:
  - 3696-3790
captured_at: 2026-06-15T17:44:41Z
---

# Episode: ADR-0057 unified ingest chokepoint — project_accepted_event replaces verify_and_persist as the dispatch+notify seam

## Prior State

verify_and_persist was the single chokepoint that persisted events AND fired both NIP-parser EventIngestDispatcher dispatch AND app-facing KernelEventObserver notify — making it the sole point where accepted events were projected.

## Trigger

PR 2 implementation (profiles as capability-owned cache) needed cache-serve/replay paths to trigger the same dispatch+notify without re-persisting, revealing that verify_and_persist conflated persistence with projection and that profiles needed a dedicated capability-owned cache seam separate from the relay ingest path.

## Decision

Split verify_and_persist so it no longer fires dispatch+notify; create project_accepted_event as the unified helper that does dispatch + notify only on Inserted | Replaced | Ephemeral outcomes, used by both live relay ingest and replay. Duplicate (including relay echoes of locally-published events) is projection-silent, preserving D4 single-fire / read-your-writes.

## Consequences

- verify_and_persist is now admission+persist only; project_accepted_event is projection
- Profiles capability seam uses project_accepted_event for both live and replay — subsumed the separate PR 1b cache-serve fan-out
- Ephemeral events now reach both parsers and observers (ADR-0057 latent-bug fix)
- Stale comments/ADR text at ingest/mod.rs:14, :384, and ADR 0057:189-193 had to be corrected to reflect the split

## Open Tail

*(none)*

## Evidence

- transcript lines 3696-3790
