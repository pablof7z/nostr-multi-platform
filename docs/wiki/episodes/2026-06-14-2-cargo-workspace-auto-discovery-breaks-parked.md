---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: root-cause
status: superseded
subjects:
  - cargo-workspace
  - parked-crates
  - nmp-nip60
supersedes:
  - 2026-06-14-1-parked-crates-must-be-their-own
related_claims: []
source_lines:
  - 7148-7244
  - 7274-7383
  - 7638-7665
captured_at: 2026-06-14T18:32:27Z
---

# Episode: Cargo workspace auto-discovery breaks parked crates with inherited fields

## Prior State

PR #1424 claimed to make parked crates standalone-buildable by replacing `workspace = true` fields with explicit values. However, the documented `cargo build --manifest-path crates/nmp-blossom/Cargo.toml` still failed with "failed to find a workspace root," and path-dep consumers (hl) also failed.

## Trigger

hl build failure against nmp-blossom path-dep with "error inheriting edition from workspace root manifest — failed to find a workspace root"; confirmed the #1424 "standalone-buildable" claim was false by reproducing the failure with the documented standalone build command.

## Decision

Root cause: Cargo's workspace auto-discovery walks up from an excluded crate and binds it to the parent workspace that excludes it, breaking both standalone builds and path-dep resolution. For truly parked crates (nip60), fix is an empty [workspace] table that makes the crate its own workspace root (PR #1427, merged as f00736f0f). For v1 crates (blossom), un-parking entirely is the correct fix.

## Consequences

- nmp-nip60 now has an empty [workspace] table plus explicit package fields — genuinely standalone-buildable
- Any future parked crate must include an empty [workspace] table or be un-parked
- The workspace.package inheritance can be restored to `workspace = true` only after the crate is un-parked back into members

## Open Tail

- PR #1426 asks whether crates with live external consumers should really be classified as dead islands — blossom is now resolved, but the CI gap (excluded crates aren't built by default) persists for nip60

## Evidence

- transcript lines 7148-7244
- transcript lines 7274-7383
- transcript lines 7638-7665

