---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: active
subjects:
  - p1-presentation-formatting
  - nmp-core
  - sf-symbols
  - relay-diagnostics
supersedes:
  - 2026-06-19-1-aim-2-overrides-adr-precompute-raw
related_claims: []
source_lines:
  - 23-26
  - 2023-2024
captured_at: 2026-06-19T11:35:40Z
---

# Episode: Presentation formatting extracted from nmp-core kernel to shells

## Prior State

SF Symbol names (e.g. "person.fill", "heart"), English prose labels, and bech32 formatting were embedded inside platform-neutral nmp-core, violating the raw-data-out principle.

## Trigger

#1493 audit P1 identified presentation formatting baked into Rust projections (D1 violation); user directed agents to fix P1.

## Decision

All presentation formatting removed from nmp-core; shells (iOS, Android, desktop, web) now own formatting. SF-Symbol names, English labels, and relay_diagnostics projection formatting moved to shell rendering layers.

## Consequences

- nmp-core is now presentation-neutral across timeline, marmot, and relay diagnostics projections
- shells must maintain parity-consistent rendering mappings
- 5 PRs required to cover all projection surfaces (nip01, marmot, nip29, publish_outbox, relay_diagnostics)

## Open Tail

- #1546 tracks web-shell config single-source + cache→wasm follow-up

## Evidence

- transcript lines 23-26
- transcript lines 2023-2024

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-presentation-formatting-extracted-from-nmp-core.json`](transcripts/2026-06-19-1-presentation-formatting-extracted-from-nmp-core.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-presentation-formatting-extracted-from-nmp-core.json`](transcripts/raw/2026-06-19-1-presentation-formatting-extracted-from-nmp-core.json)
