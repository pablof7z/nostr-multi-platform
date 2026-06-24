---
type: episode-card
date: 2026-05-21
session: 156aa64b-42e1-4d3b-96ce-25b31fc06fec
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/156aa64b-42e1-4d3b-96ce-25b31fc06fec.jsonl
salience: architecture
status: active
subjects:
  - snapshot-projection-namespace
  - doctrine-lint-d14
supersedes: []
related_claims: []
source_lines:
  - 1422-1426
  - 1494-1513
  - 1562-1578
captured_at: 2026-06-18T05:05:38Z
---

# Episode: Snapshot projection keys renamed from bare nip17.*/nip29.* to nmp.* namespace + D14 lint

## Prior State

Projection keys used bare protocol prefixes (`nip17.*`, `nip29.*`) — inconsistent with the action_namespace convention that already enforced `nmp.*` for actions.

## Trigger

Agent C5 discovered the inconsistency while scouting for nip29→nmp renames. The bare form risked collisions and violated the established naming convention.

## Decision

Renamed all projection keys to `nmp.nip17.*` / `nmp.nip29.*` form and added a D14 doctrine-lint rule banning bare `"nip17."` / `"nip29."` literals in `apps/chirp/` Rust source going forward.

## Consequences

- CI failures on PR #244 (`cargo test` + `AI architecture signoff`) — needs triage on master
- Swift mirror (`.convert` output) needs corresponding renames
- Future projection keys must follow the `nmp.<protocol>.<noun>` pattern or D14 will flag them

## Open Tail

- CI failures on merged PR #244 need investigation on master
- Swift-side projection key consumers must be updated to match

## Evidence

- transcript lines 1422-1426
- transcript lines 1494-1513
- transcript lines 1562-1578
