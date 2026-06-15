---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: root-cause
status: active
subjects:
  - parked-crates
  - cargo-workspace
  - nmp-nip60
supersedes:
  - 2026-06-14-2-parked-crate-standalone-buildability-requires-empty
related_claims: []
source_lines:
  - 7147-7383
captured_at: 2026-06-14T20:49:43Z
---

# Episode: Cargo auto-discovery breaks parked crates — empty [workspace] table required

## Prior State

PR #1424 claimed parked crates were 'standalone-buildable' by replacing workspace-inherited fields (edition, version, license, repository) with explicit values. But the documented cargo build --manifest-path still failed with 'failed to find a workspace root'. Path-dep consumers (hl) also broke identically.

## Trigger

hl build failure exposed the gap: cargo build against nmp-blossom as a path-dep produced 'error inheriting edition from workspace root manifest — failed to find a workspace root'. Diagnosis revealed Cargo's workspace auto-discovery walks UP the directory tree from a parked crate and binds it to the monorepo root that excludes it, regardless of whether [package] fields are explicit.

## Decision

Add an empty [workspace] table to each parked crate, making it its own workspace root and stopping auto-discovery. De-inheriting [package] fields alone is insufficient — only an explicit [workspace] table prevents Cargo from walking up to the excluding root. PR #1427 merged (SHA f00736f0f).

## Consequences

- nmp-nip60 is now genuinely standalone-buildable via the empty [workspace] table
- #1424's false 'standalone-buildable' claim corrected
- Future parked crates must carry an empty [workspace] table to be buildable
- The fix was later reversed for nmp-blossom when it was un-parked back into the workspace (its [workspace] table removed, inheritance restored)

## Open Tail

- nmp-wallet-poc (also parked) may need the same [workspace] table treatment if anyone tries to build it standalone

## Evidence

- transcript lines 7147-7383
