---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - ingest-chokepoint-unification
  - local-publish-intent-drift
  - admission-persistence-entanglement
supersedes:
  - 2026-06-15-1-optimistic-local-echo-for-non-replaceable
related_claims: []
source_lines:
  - 126-157
  - 193-327
captured_at: 2026-06-15T08:31:49Z
---

# Episode: #1440 root cause is architectural: no single ingest chokepoint, not missing kind arms

## Prior State

Issue #1440 was understood as a missing-feature problem: `record_local_publish_intent` lacked non-replaceable kind arms (kind:1/6/7), so the proposed fix was adding `record_local_timeline_intent` per-kind — a 4th arm in the local ladder mirroring the relay ladder's arms.

## Trigger

User correction: 'this should be NATURALLY kind independent — I suspect this is a symptom of mixing concerns or mixing abstraction layers.' Two parallel investigations (Opus agent + codex exec) launched to test the hypothesis.

## Decision

Confirmed: #1440 is a symptom of an architectural bug, not a missing kind arm. Persistence, admission/relevance filtering, and projection/cache mutation are conflated and duplicated across two hand-maintained per-kind dispatch ladders (`handle_event` for relay, `record_local_publish_intent` for local) that drift. The correct fix is unification: (1) make `verify_and_persist` the single kind-agnostic chokepoint (move `notify_event_observers` inside it), (2) demote profile/contacts/timeline caches to observers fed by the chokepoint, (3) decouple admission from persistence — `should_store_event` becomes a relevance predicate consulted by projections, not gating `store.insert`, (4) tag local publishes `Provenance::Local` (unconditionally relevant → read-your-writes for all kinds for free), (5) delete `local_publish_intent.rs` entirely.

## Consequences

- `local_publish_intent.rs` should be deleted, not extended with a 4th arm — the narrower episode-card fix is explicitly recommended against as it entrenches the divergent ladder
- The timeline read-cache (`self.events`/`self.timeline`) must become an observer — it is currently a second store hard-wired into `ingest_timeline_event`, which is the core layer-mixing that prevents kind:1/6 from using `verify_and_persist` alone
- Admission gate `should_store_event` must not gate persistence or observer firing — a self-authored event is unconditionally relevant but currently fails the `timeline_authors.contains` check because a user isn't in their own follow set
- `notify_event_observers` must move inside `verify_and_persist` (gated on Inserted|Replaced|Ephemeral) so every caller (relay, local, cache-serve) collapses to one call
- The `feed_served_event` seam in `continuation.rs:210-274` already models the kind-agnostic observer-fed projection pattern — it is the third producer alongside relay and local, proving the seam should be unified
- ADR-0045 R2.1 ('single mechanism') directly endorses this unification; the existing per-kind `match event.kind { 0|3|1|6 => }` arms in `handle_event` are a latent D0 violation the unification removes
- If unification is too large for one PR, the right intermediate is steps 1+3: move `notify_event_observers` into `verify_and_persist` + decouple admission from persistence, enabling the local path to call the chokepoint with `local://publish` provenance and delete the file

## Open Tail

- Codex exec investigation still running — independent cross-check not yet available
- Migration sequencing not yet decided (full unification in one PR vs. intermediate steps 1+3 first)
- Gift-wrap (kind:1059) exclusion from local echo must be handled via parser registry, not kind literals, under the unified model

## Evidence

- transcript lines 126-157
- transcript lines 193-327
