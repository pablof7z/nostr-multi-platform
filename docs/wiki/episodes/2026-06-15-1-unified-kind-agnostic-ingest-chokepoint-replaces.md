---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - ingest-pipeline
  - event-store
  - should-store-event
  - local-publish-intent
supersedes:
  - 2026-06-15-1-persistence-relevance-entanglement-is-the-architectural
related_claims: []
source_lines:
  - 159-159
  - 193-326
  - 356-423
  - 427-431
  - 475-507
  - 556-573
  - 586-651
captured_at: 2026-06-15T08:46:46Z
---

# Episode: Unified kind-agnostic ingest chokepoint replaces dual-ladder persistence/relevance conflation

## Prior State

Two parallel per-kind ingest ladders (handle_event for relay, record_local_publish_intent for local) maintained in sync by hand, drifting on non-replaceable arms. should_store_event gated persistence for kind:1/6 based on follow-set relevance, while all other kinds persisted unconditionally via verify_and_persist. The timeline read-cache was hard-wired inside ingest_timeline_event instead of being an observer. Non-replaceable events had no local echo (#1440). The authoritative EventStore had relevance-shaped holes.

## Trigger

User rejected the per-kind arm fix for #1440, recognizing it as a symptom of mixing concerns/abstraction layers (line 159). Code investigation confirmed that persistence, admission/relevance filtering, and projection mutation are fused in ingest_timeline_event and duplicated across the two ladders. User further declared that every non-ephemeral event must be cached unconditionally (line 427).

## Decision

Architecture must have three separated layers: (1) Admission per-source before the chokepoint — relay events admitted by subscription match, local publishes admitted unconditionally, cache replay already stored; (2) One kind-agnostic ingest chokepoint (verify + persist + dispatch + notify) that every source routes through; (3) Projections as registered observers where kind and relevance finally live. should_store_event demoted to a read-time predicate consulted only by the timeline-cache observer. local_publish_intent.rs deleted. EventStore persists every validly-signed non-ephemeral event, bounded only by GC/watermark.

## Consequences

- #1440 (ghost post) and #1442 (relevance-shaped holes) dissolve — same fix for both
- Read-your-writes for all kinds falls out for free — local publishes are admitted unconditionally
- No ladder drift — one ingest path means relay echo of local publish dedups to Duplicate, observers fire once (D4)
- pre_kind3_buffer becomes unnecessary — nothing is ever dropped at ingest, late kind:3 just triggers observer re-scan
- Cache-serve/offline, projection rebuildability, and cross-session dedup floor all become structurally sound
- D0 (substrate-honest) restored — persistence no longer bakes social-app follow-set assumption into the storage layer
- ADR required before implementation per repo convention (touches D0/D4/D8, ADR-0045)
- Issue #1442 filed as architectural bug, cross-referenced to #1440

## Open Tail

- ADR draft pending user approval before implementation
- Implementation sequencing: chokepoint helper → demote caches to observers → decouple should_store_event → route both sources → delete local_publish_intent.rs
- Codex refinement: shared seam must sit below relay-frame bookkeeping in handle_event, not route local publishes through handle_event itself

## Evidence

- transcript lines 159-159
- transcript lines 193-326
- transcript lines 356-423
- transcript lines 427-431
- transcript lines 475-507
- transcript lines 556-573
- transcript lines 586-651
