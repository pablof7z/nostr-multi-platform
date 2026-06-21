---
type: episode-card
date: 2026-05-21
session: 1c093fa5-0f0e-4dee-bf38-99781e763f13
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1c093fa5-0f0e-4dee-bf38-99781e763f13.jsonl
salience: reversal
status: active
subjects:
  - bunker-dm-architecture
  - pending-sign
  - nip-46-signing
supersedes: []
related_claims: []
source_lines:
  - 3525-3532
  - 3920-3924
captured_at: 2026-06-18T04:41:52Z
---

# Episode: Phase 2 bunker DMs should extend PendingSign, not OS-thread driver

## Prior State

PR-E Phase 1 used an OS-thread driver pattern for bunker DMs (NIP-46 encrypt+sign chain). This was the implemented approach.

## Trigger

Codex review strongly recommended against OS-thread pattern for production bunker DMs, noting it will scale poorly under burst load and that extending PendingSign is the right architectural shape for Phase 2.

## Decision

Phase 2 (bunker DMs, NIP-46 signing) will NOT use the OS-thread driver pattern. PendingSign extension is the adopted direction for production bunker DM signing.

## Consequences

- PR-E Phase 2 implementation must extend PendingSign rather than spawn OS threads
- OS-thread driver pattern is now considered historical for bunker DMs
- D13 Part B's actor/ carve-out also flagged as too broad (codex review finding)

## Open Tail

- PR-E2 design conversation needed before implementation begins
- 5s timeout possibly too aggressive for slow NIP-46 encrypt+sign chain (~10s)

## Evidence

- transcript lines 3525-3532
- transcript lines 3920-3924

