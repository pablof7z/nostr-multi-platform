---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: reversal
status: superseded
subjects:
  - nmp-blossom
  - workspace-membership
  - parked-crates
supersedes:
  - 2026-06-14-1-blossom-reclassified-from-parked-dead-island
related_claims: []
source_lines:
  - 7432-7522
  - 7524-7708
captured_at: 2026-06-14T18:32:27Z
---

# Episode: Blossom reclassified from parked/post-v1 to active v1 workspace member

## Prior State

nmp-blossom was excluded from the workspace and classified as a post-v1 dead island with a comment: "PARKED — Re-activate when M10 (#998) enters scope." Consumers (hl, podcast-player) needed /tmp copy patches or vendoring to depend on it, and the documented standalone build command was broken.

## Trigger

User correction: "blossom is not post-v1 -- it's part of v1." Reinforced by discovery that blossom has zero Unsupported/todo stubs — it's complete — and that the parked configuration broke path-dep consumers (hl failed to build).

## Decision

Un-park blossom: remove from workspace exclude list, add back to members, restore workspace.package inheritance (remove the empty [workspace] table and parking comments), release as nmp-v0.7.2 (PR #1428). Blossom is now a CI-built and CI-tested v1 crate.

## Consequences

- CI now builds and tests blossom on every PR — regressions caught early
- Consumers can depend on blossom as a normal git dep; /tmp patches and vendoring workarounds become unnecessary
- nmp-nip60 and nmp-wallet-poc remain parked with the empty [workspace] table fix from #1427
- podcast-player's main branch has a competing vendored-blossom strategy that conflicts with the 0.7.2 git-dep model — owner decision needed

## Open Tail

- podcast-player PR #501 is closed/conflicting against a main that vendored blossom at 0.6.2 — owner must decide between adopting 0.7.2 git-dep (A, recommended since vendoring is now unnecessary) or keeping vendored approach (B)
- nmp-blossom description still says "PARKED" in its Cargo.toml — should be updated

## Evidence

- transcript lines 7432-7522
- transcript lines 7524-7708

