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
  - crate-packaging
supersedes:
  - 2026-06-13-4-post-v1-dead-crates-parked-as
related_claims: []
source_lines:
  - 6759-6806
  - 6810-6882
  - 6884-6919
  - 6944-6961
captured_at: 2026-06-14T17:10:22Z
---

# Episode: Excluded crates must be self-contained — workspace inheritance breaks standalone resolution

## Prior State

Crates were parked by adding to [workspace].exclude while their Cargo.toml still inherited version, edition, license, repository, and deps via workspace = true — which Cargo cannot resolve for a non-member crate. The monorepo's own documented --manifest-path escape hatch was also broken.

## Trigger

hl (Highlighter) build failed against nmp-v0.7.0 because nmp-blossom (actively used by hl via nmp_blossom::register_actions and UploadInput) couldn't parse its manifest. Confirmed the defect is monorepo-side: cargo metadata --manifest-path crates/nmp-blossom/Cargo.toml inside the monorepo itself fails identically. The release shipped with a parked crate that an external consumer depends on and that doesn't build.

## Decision

De-inherit manifest fields in both parked crates (nmp-blossom, nmp-nip60): replace workspace = true with literal values (edition = '2021', license = 'MIT', repository = literal URL, version = '0.7.1'). Preserves the parking intent (excluded from workspace build) while restoring standalone buildability and the documented --manifest-path escape hatch. Shipped as nmp-v0.7.1 hotfix (#1424).

## Consequences

- Both parked crates now resolve and compile standalone (verified: blossom 26s, nip60 13s)
- nmp-v0.7.0 was broken for all blossom consumers — hotfixed by v0.7.1
- CI cannot catch this class of defect because excluded crates aren't built; follow-up issue #1426 filed for a release gate that compiles every excluded crate with known downstream path-deps
- Any future crate moved to [workspace].exclude must also have its manifest de-inherited or it will silently break external consumers

## Open Tail

- Issue #1426 tracks adding a release/conformance CI gate for excluded-but-consumed crates
- podcast-player pin bump to v0.7.1 in progress (redirected from broken 0.7.0)

## Evidence

- transcript lines 6759-6806
- transcript lines 6810-6882
- transcript lines 6884-6919
- transcript lines 6944-6961

