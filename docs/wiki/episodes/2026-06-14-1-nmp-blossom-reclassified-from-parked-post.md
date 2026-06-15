---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: reversal
status: superseded
subjects:
  - nmp-blossom
  - workspace-classification
  - consumer-deps
supersedes:
  - 2026-06-14-1-blossom-reclassified-from-parked-post-v1
related_claims: []
source_lines:
  - 7432-7434
  - 7475-7522
  - 7640-7670
  - 7682-7710
  - 7800-7860
  - 7867-7891
captured_at: 2026-06-14T20:10:47Z
---

# Episode: nmp-blossom reclassified from parked/post-v1 to active v1 workspace member

## Prior State

nmp-blossom was classified as a 'post-v1 dead island' and parked (excluded from the workspace) since #1324, documented as having Err(Unsupported) stubs and awaiting M10/#998 to reactivate. Consumers (hl, podcast-player) required workarounds: /tmp blossom [patch] blocks or vendor/nmp-blossom directories.

## Trigger

User correction at line 7432: 'blossom is not post-v1 -- it's part of v1.' Investigation confirmed blossom has zero Unsupported stubs and is a complete, externally-consumed v1 crate. The original #1324 parking misjudged it from in-repo evidence alone, ignoring live external consumers.

## Decision

Un-park nmp-blossom: move from workspace exclude to workspace members, restore manifest workspace inheritance (edition/version/license/repository), remove the standalone [workspace] table added by #1427, correct the parked-crate comment block. Released as nmp-v0.7.2. Podcast-player migrated from vendored-blossom model to clean 0.7.2 git-dep (deleted vendor/nmp-blossom, removed [patch] block).

## Consequences

- CI now builds and tests nmp-blossom on every PR — the gap that let 0.7.0 ship broken for blossom consumers is closed
- Consumers can depend on nmp-blossom as a normal git dep without /tmp patches or vendoring
- podcast-player dropped vendor/nmp-blossom and [patch] block entirely (PR #506 merged)
- hl resolves blossom as a normal workspace member (verified green at 0.7.2)
- nmp-v0.7.2 released with blossom as a first-class member
- The parked-crate comment in root Cargo.toml now correctly lists only nip60/wallet-poc as parked

## Open Tail

- #1426 question remains: should a release gate compile excluded-but-consumed crates to prevent future parking misclassifications from shipping broken?
- win-the-day listed as NMP consumer in docs but checkout has zero NMP linkage — needs doc reconciliation

## Evidence

- transcript lines 7432-7434
- transcript lines 7475-7522
- transcript lines 7640-7670
- transcript lines 7682-7710
- transcript lines 7800-7860
- transcript lines 7867-7891

