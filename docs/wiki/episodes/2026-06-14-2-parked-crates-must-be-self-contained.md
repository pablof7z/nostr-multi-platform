---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-blossom
  - nmp-nip60
  - workspace-exclude
  - cargo-packaging
supersedes:
  - 2026-06-14-2-excluded-crates-must-be-self-contained
related_claims: []
source_lines:
  - 6762-6835
  - 7172-7267
captured_at: 2026-06-14T17:27:33Z
---

# Episode: Parked crates must be self-contained — workspace inheritance breaks excluded consumers

## Prior State

Crates parked into [workspace].exclude (nmp-blossom, nmp-nip60) still used workspace = true inheritance for version/edition/license/repository. It was assumed they were buildable via the documented cargo build --manifest-path escape hatch and consumable as git-rev or path-deps.

## Trigger

Consumer sweep: hl's build failed with 'failed to find a workspace root / error inheriting edition' when path-depping nmp-blossom; even the monorepo's own documented standalone build command failed identically. Podcast-player's /tmp blossom patch was a workaround for the same defect.

## Decision

De-inherit workspace fields in parked crates, replacing with explicit literal values so they are genuinely standalone-buildable. nmp-v0.7.1 (#1424) shipped the first fix; a follow-up PR was dispatched when #1424 proved insufficient for the path-dep case.

## Consequences

- CI cannot catch this class of defect because it excludes these crates from the build — issue #1426 filed for a release gate that compiles excluded-but-consumed crates.
- Podcast-player must maintain a /tmp copy workaround until blossom is re-included or fully standalone.
- The deeper question was raised: should a crate with live external consumers (blossom) be classified as a 'post-v1 dead island' at all?

## Open Tail

- The path-dep case for nmp-blossom may still be broken after #1424; a follow-up agent was dispatched to verify and complete the standalone-buildability fix.
- hl auto-adopts once its monorepo source advances to include the parked-crate fix.

## Evidence

- transcript lines 6762-6835
- transcript lines 7172-7267

