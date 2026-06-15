---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: reversal
status: active
subjects:
  - nmp-blossom
  - workspace-membership
  - nmp-v0.7.2
supersedes:
  - 2026-06-14-1-nmp-blossom-reclassified-from-parked-post
  - 2026-06-14-1-blossom-un-parked-from-dead-island
related_claims: []
source_lines:
  - 7432-7712
  - 7807-7862
  - 7908-7925
captured_at: 2026-06-14T20:49:43Z
---

# Episode: Blossom restored as v1 workspace member, not parked

## Prior State

nmp-blossom was parked — excluded from the workspace and classified as 'post-v1 (M10/#998)' in the exclude block comments. This forced consumers (hl, podcast-player) into fragile workarounds: /tmp [patch] redirects, vendored copies, or path-dep builds that silently failed against the excluded crate's broken manifest.

## Trigger

User correction at two points: (1) 'blossom is not post-v1 — it's part of v1' (line 7432), and (2) 'blossom is not parked — I never said it should be parked — it should be part of v1!' (line 7908). The prior parking was a misjudgment based on in-repo evidence alone, ignoring that hl and podcast-player actively ship blossom.

## Decision

Un-park nmp-blossom: move from workspace.exclude to workspace.members, remove the standalone [workspace] table that was added while parked, restore workspace field inheritance, release as nmp-v0.7.2. CI now builds and tests blossom like every other v1 crate.

## Consequences

- CI compiles and tests nmp-blossom on every PR — it can no longer silently rot
- podcast-player migrated to clean 0.7.2 git-dep model: vendor/nmp-blossom directory deleted, [patch] block removed, blossom resolves as normal git dep (PR #506 merged)
- nmp-feedback merged at 0.7.2 (PR #1, SHA 857dedf45)
- hl verified green at 0.7.2 with blossom resolving as normal path member (auto-adopts when monorepo checkout advances past 45ac8c3e4)
- Only nmp-nip60 and nmp-wallet-poc remain parked

## Open Tail

- CI blind spot remains for excluded crates that have external consumers (nip60 if one appears) — no gate compiles them

## Evidence

- transcript lines 7432-7712
- transcript lines 7807-7862
- transcript lines 7908-7925
