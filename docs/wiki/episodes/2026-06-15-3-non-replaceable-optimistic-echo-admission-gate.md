---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - optimistic-local-echo
  - non-replaceable-ryw
  - admission-gate
  - issue-1440
supersedes:
  - 2026-06-15-4-admission-gate-blocks-naive-optimistic-echo
related_claims: []
source_lines:
  - 5-19
  - 124-148
captured_at: 2026-06-15T15:26:03Z
---

# Episode: Non-replaceable optimistic echo — admission gate crux and double-fire flaws in issue #1440

## Prior State

Only replaceable events (kind:0/3/10002) got read-your-writes echo via record_local_publish_intent. Non-replaceables (kind:1 notes, kind:6 reposts, kind:7 reactions) were invisible locally until relay echo-back — 'ghost post' UX.

## Trigger

Issue #1440 filed. Research into actual code revealed the issue's suggested fix has two critical flaws and misses the central gate problem.

## Decision

Planned implementation adds `record_local_timeline_intent` arm split by relay ingest path: timeline kinds (1/6) bypass should_store_event and call ingest_timeline_event directly (with timeline read-cache append + single observer fire); non-timeline non-replaceable kinds (7+) call verify_and_persist + notify_event_observers exactly mirroring the relay arm. The issue's suggested fix of calling both functions would cause double store-insert, double dispatcher fan-out, and double observer fire (violating D4 single-fire invariant).

## Consequences

- Fresh root notes from self will not be silently dropped — the admission gate (timeline_authors.contains) is bypassed for local-publish intent
- No double-insert or double-observer-fire: each kind uses exactly one ingest arm matching its relay counterpart
- Addressable RYW for self-authored events requires special handling at the projection/observer layer (active account interest vs follow-set membership)

## Open Tail

- Implementation not yet started — plan is documented but PRs are blocked behind current PR 2/3 landing
- Stress harness scenario catalog includes 14 HIGH-value edge cases for this (addressable RYW + Superseded-silent, kind:5 deletes unprojecting targets, NIP-40 expiry, bad-sig no-poison, etc.)

## Evidence

- transcript lines 5-19
- transcript lines 124-148
