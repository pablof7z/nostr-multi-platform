---
type: episode-card
date: 2026-07-03
session: 04745411-a0c1-4523-ac83-71dc983f410b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/04745411-a0c1-4523-ac83-71dc983f410b.jsonl
salience: direction
status: active
subjects:
  - rc-release
  - release-pipeline
  - v1-release-train
  - tag-triggered-ci
supersedes: []
related_claims: []
source_lines:
  - 1376-1404
  - 1419-1430
  - 2080-2085
captured_at: 2026-07-03T09:41:42Z
---

# Episode: RC release strategy confirmed and release pipeline validated end-to-end

## Prior State

No explicit release strategy (rc vs straight to 1.0.0); the tag-triggered release-readiness.yml workflow had never fired in the repo's history.

## Trigger

User directive: 'yes, stick to rc releases — start with the crate' (line 1430). Earlier, the throwaway rc rehearsal tag nmp-v1.0.0-rc.1 was pushed to validate the pipeline.

## Decision

Adopt rc-tagged releases as the strategy, starting with crates.io. The throwaway nmp-v1.0.0-rc.1 tag validated release-readiness.yml end-to-end: both CI jobs went green including the wasm package dry-run that fails locally on macOS.

## Consequences

- First-ever tag-triggered CI run in repo history passed (release manifest + package dry-run, OPFS conformance)
- Local macOS wasm32 build failure confirmed as toolchain-only gap, not a real gate problem
- Throwaway tag, branch, and worktree fully cleaned up — zero trace on GitHub or locally
- Real rc publish pending: 62-crate topological order computed, npm auth verified (whoami → pablof7z), @nmpis org confirmed
- Crates.io names are irreversible once published — assistant stopped before real publish awaiting final go-ahead

## Open Tail

- Real 1.0.0-rc.1 publish not yet executed; version-pin PR not yet opened; decision needed on whether to land pin fix separately or fold into rc bump

## Evidence

- transcript lines 1376-1404
- transcript lines 1419-1430
- transcript lines 2080-2085

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-4-rc-release-strategy-confirmed-and-release.json`](transcripts/2026-07-03-4-rc-release-strategy-confirmed-and-release.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-4-rc-release-strategy-confirmed-and-release.json`](transcripts/raw/2026-07-03-4-rc-release-strategy-confirmed-and-release.json)
