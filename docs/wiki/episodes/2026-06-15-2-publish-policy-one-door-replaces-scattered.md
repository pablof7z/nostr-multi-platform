---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - publish-policy-one-door
  - publish-behavior-classification
supersedes: []
related_claims: []
source_lines:
  - 2515-2557
captured_at: 2026-06-15T14:00:30Z
---

# Episode: Publish policy one-door replaces scattered kind literals (Workstream C)

## Prior State

Publish routing used scattered raw kind literals — `if kind == 0` and `if kind == 3` at `publish/action.rs:250,256` — to gate reserved-builder behavior. Policy was implicit, scattered, and had no regression gate against reintroduction of raw kind comparisons.

## Trigger

Workstream C directive to replace scattered kind-literal guards with a typed classification table, as the publish-side mirror of the ingest-side unified chokepoint (ADR-0057).

## Decision

New `publish/policy.rs` module with `classify_publish_behavior(kind) -> PublishBehavior` as the single declared policy table — the only function permitted to compare a publish kind to a named `crate::kinds::KIND_*` constant. Four variants: `ReservedBuilderOnly(ReservedKind)` (kind:0/3, raw publish refused with typed rejection), `PrivateFailClosed` (kind:1059/14, D10 Explicit-only invariant), `DiscoveryIndexable` (relay/DM-relay/mute/blocked-relay lists + 10000–19999), `PublicRoutable` (default for notes/reactions/addressables/custom). Raw `kind == N` literals in `action.rs` replaced by single `classify_publish_behavior(kind).reserved_builder()` consult. Regression gate: source scan asserting `publish/action.rs` contains no raw `kind == N` literal guard, plus 0–40000 kind sweep locking reserved-builder set to exactly {0, 3}.

## Consequences

- Publish-kind policy visible in one place (single source of truth for classification)
- Behavior byte-preserved: rejection wording unchanged (ReservedKind::raw_publish_rejection)
- Resolver routing remains in nmp-router to avoid D0 dependency inversion (DiscoveryIndexable/PrivateFailClosed declared in table but enforcement is resolver-side)
- Doctrine lint gate prevents reintroduction of scattered kind literals in routing
- Ingest-path kind:0/3 literal in local_publish_intent.rs:52 correctly left untouched (PR 1 deletes the entire file)

## Open Tail

- Workstream F will formalize doctrine gates banning store.insert/notify_event_observers outside the chokepoint
- Auto resolution and discovery-indexer fan-out stay in nmp-router's OutboxResolver (Layer 2) — not yet migrated

## Evidence

- transcript lines 2515-2557
