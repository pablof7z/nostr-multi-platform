---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: active
subjects:
  - known-signers
  - signer-catalog
  - codegen-drift-ci
supersedes:
  - 2026-06-19-4-known-signers-catalog-becomes-rust-sole
related_claims: []
source_lines:
  - 28-29
  - 1687-1690
  - 1938-1939
captured_at: 2026-06-19T11:51:39Z
---

# Episode: Known-signers Rust source-of-truth enforced by codegen

## Prior State

Known-signers table duplicated across Swift/Kotlin/web and already drifted: Amber, "Nostr Connect", and nostrconnect were absent from some native copies despite a Rust source of truth existing.

## Trigger

#1493 audit P4 finding: native code holding policy with known-signers already drifted.

## Decision

Rust KnownSignerApp catalog is the single source of truth. `nmp gen signer-catalog` generates native Kotlin×3/Swift×2 lists + AndroidManifest <queries> + iOS plist scheme checks from the catalog. Codegen-drift CI enforces parity; hand-parse gate retired.

## Consequences

- Native drift is impossible by construction — codegen regenerates all native lists from the Rust catalog
- `nmp gen signer-catalog --check` runs in CI as a drift gate
- Phased delivery (PR2a: catalog + parity gate → PR2b: full codegen) for reviewability and collision-avoidance
- 2 PRs merged: #1567 (catalog + gate), #1576 (codegen + retire hand-parse)

## Open Tail

*(none)*

## Evidence

- transcript lines 28-29
- transcript lines 1687-1690
- transcript lines 1938-1939

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-6-known-signers-rust-source-of-truth.json`](transcripts/2026-06-19-6-known-signers-rust-source-of-truth.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-6-known-signers-rust-source-of-truth.json`](transcripts/raw/2026-06-19-6-known-signers-rust-source-of-truth.json)
