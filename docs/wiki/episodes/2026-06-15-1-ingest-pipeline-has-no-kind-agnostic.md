---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - ingest-chokepoint-unification
  - local-publish-intent-deletion
  - admission-persistence-separation
supersedes:
  - 2026-06-15-1-1440-root-cause-is-architectural-no
related_claims: []
source_lines:
  - 159-162
  - 193-313
  - 356-424
captured_at: 2026-06-15T08:35:04Z
---

# Episode: Ingest pipeline has no kind-agnostic chokepoint — two parallel per-kind ladders drift

## Prior State

Two hand-maintained per-kind dispatch ladders exist: `handle_event` (relay events) and `record_local_publish_intent` (local events). They are synced by hand and have drifted — non-replaceable arms (kind:1/6/7) exist only on the relay ladder. `ingest_timeline_event` fuses persistence, admission/relevance filtering (`should_store_event`), and timeline read-cache projection into one function. Kind:1/6 are uniquely relevance-gated before store insert, while all other kinds persist unconditionally on valid signature — an asymmetry with no principled justification.

## Trigger

User rejected per-kind arm fix ('this should be NATURALLY kind independent -- completely irrelevant if its a kind:0, a kind:1 or whatever the fuck -- I suspect this is a symptom of mixing concerns or mixing abstraction layers'), then independently confirmed the architectural smell upon seeing `should_store_event` ('we only cache events if the author is in the user's follow set? that literally makes no sense'). Dual independent investigations (Opus + codex) commissioned to confirm the root cause hypothesis.

## Decision

The architectural bug is named: there is no single kind-agnostic ingest chokepoint. The correct fix is: (1) make `verify_and_persist` the universal chokepoint by moving `notify_event_observers` inside it, (2) demote `self.profiles`, `self.seed_contacts`/`timeline_authors`, and the timeline read-cache (`self.events`/`self.timeline`) to registered observers rather than inline mutations, (3) separate admission/relevance from persistence — persist unconditionally for any validly-signed event the kernel chose to ingest, decide timeline membership at the projection layer only, (4) tag local publishes with `Provenance::Local` treated as unconditionally relevant, (5) delete `local_publish_intent.rs`. A narrower symptom-fix (adding a 4th arm) was explicitly recommended against as it entrenches the divergent ladder.

## Consequences

- Read-your-writes for all kinds falls out for free — no per-kind arm needed
- The kind:1/6 asymmetry (only kinds relevance-gated before store insert) is dissolved
- ADR-0045 R2.1 'single mechanism' rule is the authority invoked for this unification
- `should_store_event` becomes a pure relevance predicate consulted by the timeline-cache observer only, not a gate on persistence
- D4 single-writer dedup is preserved — relay echo of locally-echoed event dedups to Duplicate without double-firing observers
- D0 substrate-honest constraint: the chokepoint must not match on kind literals; use behavioral predicates (`is_replaceable`, `follow_feed_kinds.contains`, parser registry)
- The existing narrower episode-card decision (add `record_local_timeline_intent` arm) is superseded and recommended against

## Open Tail

- Codex independent investigation still running — may surface additional findings or confirm/differ on migration path
- Migration order if full unification is too large for one PR: step 1 (move `notify_event_observers` into `verify_and_persist`) + step 3 (decouple admission from persistence) as the right intermediate
- Gift-wraps (kind:1059) must stay excluded from local echo — handle via parser registry, not kind literal
- Mailbox/DM transition observers (`on_mailbox_changed`/`on_dm_relays_changed`) currently in wildcard arm only — need consideration in unification

## Evidence

- transcript lines 159-162
- transcript lines 193-313
- transcript lines 356-424
