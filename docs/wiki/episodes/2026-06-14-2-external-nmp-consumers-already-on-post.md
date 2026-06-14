---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: root-cause
status: active
subjects:
  - external-consumers
  - podcast-player
  - hl
  - win-the-day
supersedes: []
related_claims: []
source_lines:
  - 6434-6487
captured_at: 2026-06-14T15:39:42Z
---

# Episode: External NMP consumers already on post-keystone API — no migration needed

## Prior State

The task brief assumed external NMP consumer apps (podcast-player, hl, win-the-day) were pinned at old pre-keystone revs and would need a large breaking migration to absorb the K1/K2/K3 API changes (register-by-value, signer-session port, kernel_mut removal, etc.).

## Trigger

Recon of actual app repos on disk revealed the premise was stale.

## Decision

No structural migration is needed. Both real consumers are already fully migrated to the post-keystone API: podcast-player uses register-by-value, nmp_defaults, per-app signer init (zero hits on removed symbols); hl path-tracks the monorepo by construction. win-the-day is not an NMP consumer at all (pure SwiftUI, zero NMP linkage) — the external-consumers doc listing it is stale.

## Consequences

- podcast-player: routine pin bump of 6 NMP revs (+ nmp-feedback) to master; refresh /tmp/nmp-at-<rev> blossom patch directory
- hl: trivial rebuild against live checkout; verify parked nmp-blossom/nmp-nip60 path deps still resolve
- win-the-day: handed back to owner as doc-reconciliation note, not a code task
- The 'adopt the whole keystone series' risk does not exist for any consumer

## Open Tail

- Update external-consumers.md to fix stale rev (104c3f76), nmp-app-template→nmp-defaults naming, and win-the-day mislisting

## Evidence

- transcript lines 6434-6487

