---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: root-cause
status: active
subjects:
  - parked-crates
  - nmp-nip60
  - cargo-workspace-discovery
supersedes:
  - 2026-06-14-2-parked-crate-cargo-workspace-auto-discovery
related_claims: []
source_lines:
  - 7156-7244
  - 7383-7384
  - 7638-7665
captured_at: 2026-06-14T20:10:47Z
---

# Episode: Parked crate standalone-buildability requires empty [workspace] table (not just de-inherited fields)

## Prior State

#1424 de-inherited [package] fields (edition, version, license, repository) from workspace in parked crates, claiming this made them 'standalone-buildable.' In reality, Cargo's workspace auto-discovery still walked up the directory tree and bound the excluded crate to the monorepo root — producing 'current package believes it's in a workspace when it's not' and breaking field inheritance for path-dep consumers.

## Trigger

hl build failure diagnosed at line 7156: 'failed to find a workspace root' when consuming nmp-blossom as a path-dep. Confirmed the documented standalone build command (cargo build --manifest-path crates/nmp-blossom/Cargo.toml) also fails with the same error, proving #1424's claim was false.

## Decision

Add an empty [workspace] table to each parked crate's Cargo.toml, making it its own workspace root. This stops Cargo's auto-discovery from walking up to the excluding monorepo root, allowing inherited workspace fields to resolve locally. Applied to nmp-nip60 (the only crate that remains genuinely parked after blossom was un-parked).

## Consequences

- nmp-nip60 is genuinely standalone-buildable — the documented cargo build --manifest-path command now works honestly
- Pattern established: any future parked crate must carry an empty [workspace] table, not just de-inherited package fields
- hl's path-dep resolution works once the monorepo checkout includes the fix commit

## Open Tail

- The empty [workspace] table pattern is fragile — if a parked crate is later un-parked, the table must be removed (as was discovered when un-parking blossom which still had it, breaking workspace inheritance)

## Evidence

- transcript lines 7156-7244
- transcript lines 7383-7384
- transcript lines 7638-7665

