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
  - workspace-exclude-packaging
  - cargo-workspace-auto-discovery
supersedes:
  - 2026-06-14-2-parked-crates-must-be-self-contained
related_claims: []
source_lines:
  - 6762-6810
  - 6834-6895
  - 7156-7210
  - 7228-7252
  - 7365-7383
captured_at: 2026-06-14T17:49:52Z
---

# Episode: Parked crates must be their own workspace roots — Cargo auto-discovery binds excluded crates to the parent

## Prior State

Parked crates (nmp-blossom, nmp-nip60) were excluded from the workspace via `[workspace].exclude` but still inherited `version`/`edition`/`license`/`repository` via `{ workspace = true }`. Even after de-inheriting those fields (#1424), Cargo's workspace auto-discovery would walk up from the excluded crate's directory and bind it to the parent workspace — which refuses to resolve it as a member. The documented escape hatch (`cargo build --manifest-path crates/nmp-blossom/Cargo.toml`) was broken, and any external consumer path-depping on a parked crate would fail with 'failed to find a workspace root.'

## Trigger

Consumer sweep: hl's build broke against nmp-v0.7.0 because its path-dep on nmp-blossom hit 'error inheriting edition … failed to find a workspace root.' The monorepo's own standalone build command for the parked crate produced the identical error. CI never caught this because it excludes those crates from the default build.

## Decision

Each parked crate now has an empty `[workspace]` table in its Cargo.toml, making it its own workspace root. This prevents Cargo auto-discovery from walking up to the parent workspace that excludes it. The `[package]` fields were also de-inherited to explicit literals (edition = "2021", version = "0.7.1", etc.). PR #1427 merged to master at f00736f0f.

## Consequences

- hl's path-dep on nmp-blossom resolves without workarounds; podcast-player can drop its /tmp blossom-patch mechanism
- The documented `cargo build --manifest-path crates/nmp-blossom/Cargo.toml` command now actually works
- nmp-v0.7.1 (tag 92fdfca32) de-inherited the package fields; nmp-v0.7.0's release shipped with the parked crates broken for any consumer
- CI still does not build excluded crates — a release could ship broken parked crates again. Issue #1426 filed to add a gate that compiles every excluded crate a known downstream path-deps
- win-the-day was listed as an NMP consumer but has zero NMP linkage; doc corrected in #1423

## Open Tail

- Issue #1426: should a crate with live external consumers (blossom) be classified as a 'post-v1 dead island' at all? Add a release-gate that compiles excluded-but-consumed crates
- Owner must advance hl's checkout past f00736f0f for the fix to take effect (owner-managed branch, deliberately not forced)
- podcast-player's /tmp blossom patch can be dropped as cleanup now that parked crates resolve standalone

## Evidence

- transcript lines 6762-6810
- transcript lines 6834-6895
- transcript lines 7156-7210
- transcript lines 7228-7252
- transcript lines 7365-7383

