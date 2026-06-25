---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - ci
  - nmp-app-chirp
  - wire-fixtures
  - golden-tests
supersedes: []
related_claims: []
source_lines:
  - 1149-1158
  - 1162-1166
captured_at: 2026-06-18T21:02:14Z
---

# Episode: CI blind spot: apps/* crate tests not compiled, allowing wire-shape breaks to merge green

## Prior State

CI's cargo test job does NOT compile the apps/* crate tests (including nmp-app-chirp). This allowed PR #1528 to merge fully green while nmp-app-chirp had E0560 errors referencing removed AuthorDisplay.npub and Nip10ReplyAttribution fields. Both manual review and CI missed it; only a post-hoc codex review caught the breakage.

## Trigger

Codex post-hoc review of merged P1 PRs found that apps/chirp/crates/nmp-app-chirp/tests/typed_feed_parity.rs still referenced removed fields (E0560), which CI structurally cannot detect.

## Decision

Filed #1553 to add apps/* crate test compilation to CI. Immediate mitigation: codex review before every PR becomes a hard gate. The broken test was fixed via follow-up PR #1551 (3-line fixture fix, merged 0eff45f56).

## Consequences

- Any future wire-shape change that breaks apps/* tests will continue to merge green until #1553 is fixed
- Codex review is now enforced as a hard pre-merge gate for all campaign PRs
- The P1 agent adopted hardened discipline: regenerate ALL golden fixtures (including triplicated copies) and compile BOTH shells before pushing

## Open Tail

- #1553 (add apps/* test compilation to CI) not yet implemented

## Evidence

- transcript lines 1149-1158
- transcript lines 1162-1166

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-6-ci-blind-spot-apps-crate-tests.json`](transcripts/2026-06-18-6-ci-blind-spot-apps-crate-tests.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-6-ci-blind-spot-apps-crate-tests.json`](transcripts/raw/2026-06-18-6-ci-blind-spot-apps-crate-tests.json)
