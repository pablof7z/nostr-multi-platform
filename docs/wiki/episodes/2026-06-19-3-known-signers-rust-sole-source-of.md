---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - known-signers
  - signer-catalog
  - codegen-drift-ci
supersedes:
  - 2026-06-19-2-known-signers-rust-sole-source-of
related_claims: []
source_lines:
  - 1687-1690
  - 1932-1954
captured_at: 2026-06-19T06:25:53Z
---

# Episode: Known-signers: Rust sole source of truth, codegen-enforced parity with native shells

## Prior State

Known-signers tables duplicated and already drifted across Swift/Kotlin/web despite a Rust source of truth existing. Hand-maintained native copies diverged from Rust (Amber/Nostr Connect/nostrconnect label drift).

## Trigger

P4 audit finding: known-signers Rust↔native drift, already diverged; P9 PR2 phased design review

## Decision

Rust owns a single KnownSignerApp catalog (single source of truth). Native lists (Kotlin×3, Swift×2) + AndroidManifest <queries> + iOS plist schemes are GENERATED via `nmp gen signer-catalog --check`. codegen-drift CI enforces parity — hand-parse gate retired. nip46_onboarding derives from the catalog.

## Consequences

- Eliminates native drift permanently — any divergence caught by CI
- Rust is the sole writer; native shells consume generated code
- Hand-maintained native signer lists replaced by generated siblings
- VendorDriftGate reworked to use codegen output

## Open Tail

*(none)*

## Evidence

- transcript lines 1687-1690
- transcript lines 1932-1954

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-3-known-signers-rust-sole-source-of.json`](transcripts/2026-06-19-3-known-signers-rust-sole-source-of.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-3-known-signers-rust-sole-source-of.json`](transcripts/raw/2026-06-19-3-known-signers-rust-sole-source-of.json)
