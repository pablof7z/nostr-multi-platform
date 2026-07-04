---
type: episode-card
date: 2026-07-03
session: 04745411-a0c1-4523-ac83-71dc983f410b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/04745411-a0c1-4523-ac83-71dc983f410b.jsonl
salience: root-cause
status: active
subjects:
  - version-pinning
  - crates-io-publish
  - internal-deps
  - topo-sort
supersedes: []
related_claims: []
source_lines:
  - 1434-1447
  - 1449-1455
  - 1556-1561
  - 1949-1955
  - 2064-2077
captured_at: 2026-07-03T09:41:42Z
---

# Episode: Internal path-dep version pinning unblocks crates.io publishing

## Prior State

Zero of 394 internal nmp-* path dependencies across the workspace carried a version = field — crates.io requires all dependencies (including path deps) to declare a version for published crates.

## Trigger

Attempted cargo publish --dry-run during rc preparation; discovered nmp-kinds failed with 'no matching package named nmp-ownership found' because crates.io cannot resolve bare path deps.

## Decision

Added version = "0.8.4" to 320 internal path-dep lines across 59 crate manifests (scoped to [dependencies] and [build-dependencies] only — dev-dependencies are auto-stripped by cargo and including them created false topo-sort cycles). Also fixed the root [workspace.dependencies] nmp-ownership entry.

## Consequences

- cargo metadata resolves cleanly; full 62-crate topological publish order computed with zero cycles
- cargo publish --dry-run on nmp-ownership (first in publish order) passes end-to-end through packaging, verification, compilation, and upload-abort
- Publish order: nmp-ownership → nmp-codegen → ... → nmp-uniffi-support (62 crates total)
- Script bug found and fixed: tomlkit inline-table rebuild dropped the space after { — normalized in post-processing
- Initial over-scoping (including dev-dependencies) created false cycles from test-fixture cross-deps; corrected by restricting to non-dev sections

## Open Tail

- PR not yet opened for the version-pin fix; assistant proposed landing it separately from the rc version bump

## Evidence

- transcript lines 1434-1447
- transcript lines 1449-1455
- transcript lines 1556-1561
- transcript lines 1949-1955
- transcript lines 2064-2077

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-3-internal-path-dep-version-pinning-unblocks.json`](transcripts/2026-07-03-3-internal-path-dep-version-pinning-unblocks.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-3-internal-path-dep-version-pinning-unblocks.json`](transcripts/raw/2026-07-03-3-internal-path-dep-version-pinning-unblocks.json)
