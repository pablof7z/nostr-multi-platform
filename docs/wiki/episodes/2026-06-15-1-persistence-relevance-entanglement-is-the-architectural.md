---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - ingest-pipeline
  - event-store
  - should-store-event
  - local-publish-intent
supersedes:
  - 2026-06-15-1-ingest-pipeline-has-no-kind-agnostic
related_claims: []
source_lines:
  - 126-157
  - 159-159
  - 193-327
  - 356-423
  - 427-474
captured_at: 2026-06-15T08:40:03Z
---

# Episode: Persistence-relevance entanglement is the architectural root cause of local-echo failure and store integrity holes

## Prior State

Non-replaceable local echo (#1440) was assumed to be a missing kind arm — kind:1/6/7 lacked a `record_local_timeline_intent` entry in `local_publish_intent.rs`. The proposed fix added per-kind arms with a `local://publish` sentinel admission clause. Meanwhile, `should_store_event` (timeline.rs:299) gated persistence for kind:1/6 by follow-set membership (`timeline_authors.contains`), while all other kinds (kind:0, kind:3, kind:7, replaceables) persisted unconditionally via `verify_and_persist`. The two parallel per-kind dispatch ladders (`handle_event` for relay, `record_local_publish_intent` for local) were hand-synced and drifting.

## Trigger

User rejected the per-kind arm fix as a symptom patch ("this should be NATURALLY kind independent" — line 159), hypothesizing mixing of concerns/abstraction layers. Code investigation confirmed: `ingest_timeline_event` fuses persistence (`store.insert`), admission gating (`should_store_event`), and projection mutation (timeline read-cache append) into one function; the admission gate blocks authoritative store inserts for kind:1/6 only — an asymmetry with no principled justification.

## Decision

The root cause is architectural: persistence is entangled with relevance filtering and projection mutation. The correct invariant is "persist unconditionally, filter at read time." `should_store_event` must become a pure relevance predicate consulted only by the timeline read-cache observer — never gating `store.insert`. The two parallel ingest ladders collapse to one kind-agnostic chokepoint: `verify_and_persist` (with `notify_event_observers` moved inside it), caches demoted to observers, `local_publish_intent.rs` deleted. Architectural bug #1442 filed with 10 ramifications (cache-serve holes, projection unrebuildability, re-REQ amplification, D0 social-assumption-in-storage, D9 replay hostility, etc.). #1440 cross-referenced as the local-publish half of the same root cause.

## Consequences

- Authoritative EventStore currently has relevance-shaped holes — events received but never persisted because author wasn't in follow set at ingest time
- Projections cannot be rebuilt from store; new projections permanently lose historically-dropped events
- Cache-serve / offline is lossy by construction (ADR-0045 store-first mandate violated)
- kind:1/6 are the only persistence-gated kinds; kind:7 from a stranger persists unconditionally — the asymmetry proves the gate is not a principled bound
- `pre_kind3_buffer` (10K cap) is a band-aid that silently overflows; not a fix
- ADR-0045 R2.1 (single mechanism) already mandates the unification; `feed_served_event` (continuation.rs:210-274) is the existing kind-agnostic model
- Doctrine D0 violated: social-app assumption (follow set) baked into the storage layer
- D4 single-writer dedup preserved under the new model — relay echo of locally-echoed event dedups to Duplicate, no double-fire
- Initial per-kind fix explicitly rejected as entrenching the divergent ladder; the episode card's narrower decision is superseded

## Open Tail

- Codex independent investigation still pending — verdict to be reconciled
- Migration path outlined in 5 steps but not executed (step 1: move notify_event_observers into verify_and_persist; step 3: decouple admission from persistence are the recommended first PR)
- Whether full unification or incremental steps for implementation not yet decided
- Gift-wrap (kind:1059) exclusion from local echo must be handled via parser registry, not kind literals, under the new model

## Evidence

- transcript lines 126-157
- transcript lines 159-159
- transcript lines 193-327
- transcript lines 356-423
- transcript lines 427-474
