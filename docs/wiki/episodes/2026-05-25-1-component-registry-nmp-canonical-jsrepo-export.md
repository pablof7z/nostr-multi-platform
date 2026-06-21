---
type: episode-card
date: 2026-05-25
session: 45258890-9aa6-4063-8df0-bdf7021e9f72
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/45258890-9aa6-4063-8df0-bdf7021e9f72.jsonl
salience: architecture
status: active
subjects:
  - nmp-cli
  - component-registry
  - jsrepo
supersedes: []
related_claims: []
source_lines:
  - 78-114
captured_at: 2026-06-18T05:20:04Z
---

# Episode: Component registry: nmp canonical, jsrepo export-only

## Prior State

jsrepo was listed as a future step in the registry roadmap without explicit hierarchy or source-of-truth ordering relative to nmp's offline registry

## Trigger

User's strategic review after PR 503 landed, articulating the full product model and sequencing for the shadcn-style source component registry

## Decision

nmp remains the canonical offline registry path; jsrepo is positioned solely as a compatibility/export layer that is only adopted after the registry model is proven through real app usage (steps 2–6)

## Consequences

- jsrepo integration is gated on proving the registry model in Chirp first
- No parallel jsrepo implementation until nmp's install/update/lock cycle is validated end-to-end
- Source-of-truth for component definitions stays in crates/nmp-cli/registry
- The update command (step 2) is the highest-priority next step, since it delivers the core promise of safe upstream diffing

## Open Tail

- nmp update component not yet implemented — the lock-file SHA-256 hashes exist precisely for this diff
- ContentTreeWire fixture set (step 3) is prerequisite for real iOS/Android renderer validation

## Evidence

- transcript lines 78-114

