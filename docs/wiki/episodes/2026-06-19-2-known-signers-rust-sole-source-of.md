---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - known-signers
  - codegen
  - source-of-truth
  - native-drift
supersedes:
  - 2026-06-18-2-known-signers-rust-as-codegen-enforced
related_claims: []
source_lines:
  - 1293-1296
  - 1430-1438
  - 1494-1505
captured_at: 2026-06-19T00:18:35Z
---

# Episode: Known-signers: Rust sole source of truth, codegen-enforced

## Prior State

Known-signers tables were duplicated across Swift, Kotlin (×3), and web with already-drifted values (e.g., Amber vs 'Nostr Connect' vs absent). A fragile hand-parse gate existed in PR2a.

## Trigger

#1493 P4 audit found Swift/Kotlin/web signer lists had drifted from the Rust source of truth.

## Decision

Rust catalog is the sole writer of the known-signer list. nmp-codegen generates native Kotlin×3/Swift×2 lists from dump_signer_catalog JSON. codegen-drift CI enforces parity. PR2a hand-parse gate retired.

## Consequences

- No manual native signer copies — future additions go to Rust catalog only
- codegen-drift.yml CI catches any native drift
- VendorDriftGate extended to generated Kotlin
- registry.json regenerated with correct metadata
- CI blind spot (#1553) required local cargo test -p nmp-app-chirp before every push since CI doesn't compile apps/*
- PR2a caught a real registry.json export-drift via CI before merging

## Open Tail

*(none)*

## Evidence

- transcript lines 1293-1296
- transcript lines 1430-1438
- transcript lines 1494-1505

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-2-known-signers-rust-sole-source-of.json`](transcripts/2026-06-19-2-known-signers-rust-sole-source-of.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-2-known-signers-rust-sole-source-of.json`](transcripts/raw/2026-06-19-2-known-signers-rust-sole-source-of.json)
