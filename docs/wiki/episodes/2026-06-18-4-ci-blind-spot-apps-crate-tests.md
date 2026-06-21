---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: active
subjects:
  - ci-coverage
  - nmp-app-chirp
  - wire-shape-safety
supersedes:
  - 2026-06-18-6-ci-blind-spot-apps-crate-tests
related_claims: []
source_lines:
  - 1151-1153
  - 1162-1167
captured_at: 2026-06-18T22:54:46Z
---

# Episode: CI blind spot: apps/* crate tests not compiled by default cargo test

## Prior State

CI's cargo test job does not compile the nmp-app-chirp crate's tests. Wire-shape breaks in FlatBuffers projections could merge green while leaving that crate red on master.

## Trigger

Codex post-hoc review of P1 #1528 caught a real E0560 bug (nmp-app-chirp tests referencing removed AuthorDisplay.npub / Nip10ReplyAttribution fields) that both manual review and CI missed. Master was silently red for that crate.

## Decision

Filed #1553 to add apps/* crate test compilation to CI. Enforced manual discipline: run `cargo test -p nmp-app-chirp` locally before every push until #1553 lands.

## Consequences

- Every agent now runs cargo test -p nmp-app-chirp locally before pushing
- Future wire-shape changes cannot merge green while breaking apps/* silently
- p9 caught and fixed a registry.json export-drift in PR2b via this discipline

## Open Tail

- #1553 (add apps/* test compilation to CI) not yet implemented

## Evidence

- transcript lines 1151-1153
- transcript lines 1162-1167

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-4-ci-blind-spot-apps-crate-tests.json`](transcripts/2026-06-18-4-ci-blind-spot-apps-crate-tests.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-4-ci-blind-spot-apps-crate-tests.json`](transcripts/raw/2026-06-18-4-ci-blind-spot-apps-crate-tests.json)
