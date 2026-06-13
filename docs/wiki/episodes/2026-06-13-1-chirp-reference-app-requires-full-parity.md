---
type: episode-card
date: 2026-06-13
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: product
status: active
subjects:
  - chirp-parity
  - platform-scope
supersedes: []
related_claims: []
source_lines:
  - 8719-8721
captured_at: 2026-06-13T20:26:16Z
---

# Episode: Chirp reference app requires full parity across all 3 platforms

## Prior State

Chirp's cross-platform feature parity bar was undefined — the reference app's scope (iOS, Android, desktop) was ambiguous, with no explicit commitment on how much of each platform should be supported

## Trigger

User was explicitly asked during triage: 'Chirp feature parity across iOS/Android/desktop (#1291): what's the bar? Chirp is the framework's reusability proof, not a shipping product — so its features exist to exercise NMP seams.'

## Decision

Full parity across all 3 platforms (iOS, Android, desktop) — Chirp must exercise every NMP seam on every platform

## Consequences

- Thin-shell UI tasks required per platform to reach parity
- All NMP projection/component seams must be exercised by Chirp
- #1291 opened as tracker for the full-parity sweep
- Wave 2+ work scoped to ensure no platform is a second-class citizen

## Open Tail

- Full-parity UI sweep (#1291) scheduled after Wave 2 projection work lands

## Evidence

- transcript lines 8719-8721

