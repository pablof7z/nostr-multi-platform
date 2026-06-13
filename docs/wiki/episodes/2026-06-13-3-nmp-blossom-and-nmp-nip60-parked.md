---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: reversal
status: superseded
subjects:
  - nmp-blossom
  - nmp-nip60
  - workspace-exclusion
supersedes: []
related_claims: []
source_lines:
  - 8509-8549
captured_at: 2026-06-13T20:56:22Z
---

# Episode: nmp-blossom and nmp-nip60 parked as post-v1 dead islands

## Prior State

nmp-blossom and nmp-nip60 were active workspace members, included in the default build and the release manifest (nmp-release.toml public_crates list).

## Trigger

Issue #1250 identified these crates as post-v1 dead islands: their primary action stubs return Err(Unsupported), and keeping them active violates the no-dead-code / no-false-advertising rule.

## Decision

Exclude nmp-blossom and nmp-nip60 from the workspace (Cargo.toml exclude list). Remove them from release/nmp-release.toml (both public_crates and nmp-wallet-poc from private_packages). Add PARKED markers and comments pointing to re-activation issues #998 and #1001.

## Consequences

- These crates are no longer in the default cargo build; CI release-manifest and package-dry-run gates must stay in sync with workspace membership
- Re-activation tracked via issues #998 (blossom) and #1001 (nip60)
- A duplicate PR (#1319) was also merged by another session but was harmless (stacked cleanly on #1324)
- nmp-wallet-poc also removed from private_packages as a dependency of parked crates

## Open Tail

- Re-activation of blossom/nip60 requires adding them back to workspace + release manifest

## Evidence

- transcript lines 8509-8549

