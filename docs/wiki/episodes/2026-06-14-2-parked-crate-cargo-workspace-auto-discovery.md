---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: root-cause
status: superseded
subjects:
  - cargo-workspace-auto-discovery
  - parked-crates
  - nmp-nip60
  - workspace-inheritance
supersedes:
  - 2026-06-14-2-cargo-workspace-auto-discovery-breaks-parked
related_claims: []
source_lines:
  - 7156-7383
captured_at: 2026-06-14T20:01:17Z
---

# Episode: Parked-crate Cargo workspace auto-discovery breaks inherited fields

## Prior State

#1424 claimed parked crates (nmp-blossom, nmp-nip60) were 'standalone-buildable' after de-inheriting [package] fields. The documented `cargo build --manifest-path` commands were believed to work. In reality, Cargo's workspace auto-discovery walks up the directory tree from a parked crate and binds it to the monorepo root that excludes it, causing 'failed to find a workspace root' for any path-dep consumer.

## Trigger

hl build against nmp-v0.7.1 failed with 'error inheriting edition from workspace root manifest / workspace.package.edition was not defined' (lines 7147-7165). Further diagnosis confirmed even the documented standalone build commands failed the same way.

## Decision

The load-bearing fix is an empty `[workspace]` table per parked crate, making each its own workspace root and stopping Cargo's upward auto-discovery. #1427 applied this; #1428 then superseded it for blossom (un-parked), but nip60 retains the empty [workspace] table as its fix.

## Consequences

- nip60 and wallet-poc (still parked) now genuinely standalone-buildable via the empty [workspace] table
- Path-dep consumers resolve parked crates correctly when crates carry the [workspace] marker
- The #1424 'standalone-buildable' claim was false until #1427 fixed it — documented build commands were lying
- When un-parking a crate back into the workspace, the [workspace] table must be removed (learned the hard way: blossom broke again when the table was left in during un-parking, lines 7638-7664)

## Open Tail

- Add a CI or lint gate that verifies `cargo build --manifest-path` for parked crates actually works, preventing future false standalone-buildable claims

## Evidence

- transcript lines 7156-7383

