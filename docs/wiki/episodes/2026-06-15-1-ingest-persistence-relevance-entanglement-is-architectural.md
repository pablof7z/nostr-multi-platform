---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - ingest-pipeline
  - should-store-event
  - local-publish-intent
  - eventstore-completeness
supersedes:
  - 2026-06-15-1-unified-kind-agnostic-ingest-chokepoint-replaces
related_claims: []
source_lines:
  - 124-143
  - 196-326
  - 356-424
  - 427-497
  - 556-584
captured_at: 2026-06-15T08:57:04Z
---

# Episode: Ingest persistence-relevance entanglement is architectural bug, not missing kind arm

## Prior State

Issue #1440 was understood as a missing kind arm — non-replaceable events needed a record_local_timeline_intent arm in the local-publish ladder. The should_store_event gate (timeline_authors.contains) was assumed to be a correct bounded-cache relevance mechanism for timeline events. An episode card already recorded the narrower per-arm fix.

## Trigger

Investigation of #1440's code revealed: (1) kind:1/6 are the ONLY kinds where persistence is relevance-gated — every other kind goes through verify_and_persist unconditionally; (2) the dual ingest ladders (handle_event for relay, record_local_publish_intent for local) drift because they're hand-maintained per-kind copies that never got non-replaceable arms; (3) should_store_event gates the authoritative EventStore, not just the timeline view. User confirmed the architectural scope: 'EVERY event should be cached! always! this is insane!'

## Decision

Recognized as architectural bug, not a missing kind arm. Filed #1442: 'Persistence is entangled with relevance — authoritative EventStore has relevance-shaped holes.' The correct fix is a unified kind-agnostic ingest chokepoint: admission is per-source ('did I ask for/make this?'), persistence is unconditional for admitted events, relevance filtering moves to read-time projection only. The episode card's narrower fix (4th arm) is explicitly recommended against as it entrenches the divergent ladder. local_publish_intent.rs should be deleted in the end state.

## Consequences

- Issue #1442 filed (priority:p1, category:violation), cross-referencing #1440 as the local-publish half of the same root cause
- The authoritative EventStore has relevance-shaped holes — cache-serve/offline is structurally broken, projections aren't rebuildable, cross-session dedup floor is defeated
- should_store_event's timeline_authors.contains clause is the specific bug: a social follow-set gates the authoritative store, not just the timeline view
- pre_kind3_buffer (10K cap) is a band-aid that silently overflows and becomes unnecessary once everything is persisted
- ADR needed before code (touches D0/D4/D8, ADR-0045); migration sequence: move notify_event_observers into verify_and_persist → demote caches to observers → decouple should_store_event → route both sources → delete local_publish_intent.rs
- handle_event's relay-frame bookkeeping must stay above the shared chokepoint — local publishes can't route through handle_event directly (codex refinement)
- The three-layer model (admission → chokepoint → projections-as-observers) becomes the target architecture; cache-serve/replay already proves this shape

## Open Tail

- ADR draft not yet written — user was asked but session ended before confirmation
- Episode card docs/wiki/episodes/2026-06-15-1 records a narrower decision contradicting the architectural fix — needs reconciliation
- Kind:1059 gift-wraps must stay excluded from local echo; needs handling via parser registry, not a kind literal

## Evidence

- transcript lines 124-143
- transcript lines 196-326
- transcript lines 356-424
- transcript lines 427-497
- transcript lines 556-584
