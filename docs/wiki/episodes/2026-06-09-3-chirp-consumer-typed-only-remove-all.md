---
type: episode-card
date: 2026-06-09
session: 63af4b96-d3d3-45c3-ab96-9f899beafa1b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/63af4b96-d3d3-45c3-ab96-9f899beafa1b.jsonl
salience: product
status: active
subjects:
  - typed-projections
  - chirp-consumer
  - kernel-update
  - snapshot-frame
supersedes: []
related_claims: []
source_lines:
  - 6575-6895
captured_at: 2026-06-11T23:10:26Z
---

# Episode: Chirp consumer typed-only: remove all JSON fallbacks and whole-payload decode

## Prior State

Chirp's kernel-snapshot consumer read typed sidecars first but fell back to JSON (?? snapshot?.X / ?? update.X pattern) for every field; the whole-payload KernelUpdate: Decodable struct decoded the generic Value tree

## Trigger

Producer-completeness gate proved all 7 data-carrying generic keys have typed sidecars, making JSON fallbacks unnecessary; advisor required completing the migration, not just advancing it

## Decision

Remove all ~35 JSON fallbacks, delete the KernelUpdate: Decodable struct (~678 LOC net deletion), re-home rev/metrics/lastErrorToast onto the typed SnapshotFrame envelope, migrate test harness to a genuine typed seam (setTypedSnapshotForTesting)

## Consequences

- Chirp reads zero JSON for kernel snapshots — the reference app is typed-only
- Test seam is genuinely typed (no JSON path retained), so tests prove the typed path works
- Staleness guard re-homed to typedEnvelope.rev — statically proven safe because encode_snapshot_with_envelope always writes metrics unconditionally, so typedEnvelope is never nil in production
- PR #1064 (162/0/4 ChirpTests green, stacked on #1062 + #1063)

## Open Tail

- apply() itself is compile-verified but not runtime-exercised by non-smoke tests (4 skipped NMP_SMOKE tests); static proof closes the risk but a live smoke would strengthen confidence

## Evidence

- transcript lines 6575-6895

