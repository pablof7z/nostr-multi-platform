---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: reversal
status: superseded
subjects:
  - nmp-nip60
  - nmp-blossom
  - workspace-membership
supersedes:
  - 2026-06-13-3-nmp-blossom-and-nmp-nip60-parked
related_claims: []
source_lines:
  - 9116-9183
captured_at: 2026-06-13T21:35:37Z
---

# Episode: Post-v1 dead crates parked as historical

## Prior State

nmp-nip60 and nmp-blossom were active Cargo workspace members

## Trigger

Architecture audit identified them as post-v1 dead islands with no active consumers and no path to v1 completion

## Decision

Park both crates: exclude from Cargo.toml workspace with PARKED comments; update release manifest to drop from public_crates/private_packages; primary action stubs return Err(Unsupported)

## Consequences

- No-dead-code / no-false-advertising doctrine rule no longer violated
- Workspace builds skip parked crates
- Merged via #1324 and #1319 (duplicate, harmless)

## Open Tail

*(none)*

## Evidence

- transcript lines 9116-9183

