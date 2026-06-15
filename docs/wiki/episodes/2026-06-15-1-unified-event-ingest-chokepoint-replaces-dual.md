---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - unified-ingest-chokepoint
  - read-your-writes
  - persistence-vs-relevance
  - d4-single-fire
supersedes: []
related_claims: []
source_lines:
  - 1-144
  - 2570-2600
captured_at: 2026-06-15T14:44:31Z
---

# Episode: Unified event ingest chokepoint replaces dual-path local/relay ladder

## Prior State

Two separate ingest ladders existed: `record_local_publish_intent` (local publish, only handled replaceables — kind:0/3/10002 — non-replaceables had NO echo path) and relay ingest via `handle_event`→`ingest_timeline_event`/`verify_and_persist`. `should_store_event` was a persistence gate: its primary clause `timeline_authors.contains(author)` meant a non-followed author's event was dropped before `store.insert`, creating permanent store holes. A self-authored kind:1 note was also dropped (you are not in your own follow set). Ephemerals never reached app observers (wildcard `notify_event_observers` fired only on `Inserted|Replaced`). `pre_kind3_buffer` was a band-aid for events arriving before kind:3 processing.

## Trigger

Issue #1440 — ghost-post UX: locally-published kind:1/6/7 events are invisible until relay echo (seconds or failure on flaky connection). Deep code research confirmed the dual-ladder architecture, the `should_store_event` admission gate blocking self-authored notes, and the persistence/relevance entanglement causing permanent store holes.

## Decision

Single chokepoint `ingest_accepted_event`: ALL event ingest (local publish AND relay) goes through one path. Admission is valid-sig only (no relevance gate at persistence). `should_store_event` demoted to read-time predicate for timeline projection only. Delivery (NIP-parser dispatch + observer notify) fires exactly once on `Inserted|Replaced|Ephemeral`. `record_local_publish_intent` and `pre_kind3_buffer` deleted. Publish-in-flight GC pin added to `derive_store_pin_set` so locally-accepted events survive GC pressure before relay confirmation.

## Consequences

- Read-your-writes for all event kinds: kind:1/6/7 local publishes appear immediately before any relay ACK
- D4 single-fire invariant: relay echo of a locally-published event dedups to Duplicate (no re-notify); only diagnostic relay_count bumps
- Non-followed events persist in the store but do NOT project into the follow-feed timeline (persistence ≠ relevance)
- Ephemeral events (kind:20000-29999) now reach app KernelEventObservers but are never stored
- Complete store enables lossless projection rebuild on cold restart and late-backfill when a new follow is added
- D9 future-date clamp must live in the timeline observer specifically (not in `kernel_event_from_nostr` which does not clamp)
- Duplicate relay_count bump on kind:1/6 is preserved even though Duplicate doesn't re-notify

## Open Tail

- PR 2 (profiles → ProfileLookup capability trait) — replaces the kernel-owned profile arm with an IngestParser
- PR 3 (contacts → parser + effect signal) — removes kind:3 literals from ingest path
- Workstream F (doctrine gates) — lint/CI enforcement that no scattered kind literals reappear in routing

## Evidence

- transcript lines 1-144
- transcript lines 2570-2600
