---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - known-signers
  - signer-catalog
  - codegen
  - native-drift
supersedes:
  - 2026-06-18-2-known-signers-rust-becomes-codegen-enforced
related_claims: []
source_lines:
  - 1255-1279
  - 1430-1438
captured_at: 2026-06-18T23:38:04Z
---

# Episode: Known-signers: Rust as codegen-enforced sole writer

## Prior State

Known-signers table was duplicated across Swift (2 copies), Kotlin, and web, and had already drifted from each other despite a Rust source of truth existing. No enforcement mechanism existed to keep native lists in sync with Rust.

## Trigger

Issue #1493 P4/P9 audit found that native signer lists had diverged (e.g., nostrconnect labeled 'Nostr Signer' in Swift vs. 'Signer App' elsewhere; nostrsigner→Amber label inconsistent) and the hand-maintained native copies were drifting independently.

## Decision

Adopted a phased approach (per codex consultation): PR2a establishes Rust as the single source of truth with a Rust-tied --check gate asserting native lists match the catalog; PR2b replaces hand-maintained native lists entirely with nmp-codegen emitters that generate Kotlin×3/Swift×2 lists + AndroidManifest <queries> + iOS plist schemes from the Rust catalog, enforced by codegen-drift CI.

## Consequences

- nmp-core signer_catalog.rs is now the sole writer of the known-signer catalog
- nmp gen signer-catalog --check renders KnownSigners.generated.{kt×3, swift×2} from dump_signer_catalog JSON
- codegen-drift CI gate enforces parity; hand-parse gate from PR2a retired by PR2b
- nip46_onboarding now derives from the catalog (fixed label drift: nostrconnect 'Nostr Signer'→'Nostr Connect')
- Future signer additions require only a Rust catalog entry; native code is auto-generated

## Open Tail

*(none)*

## Evidence

- transcript lines 1255-1279
- transcript lines 1430-1438

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-known-signers-rust-as-codegen-enforced.json`](transcripts/2026-06-18-2-known-signers-rust-as-codegen-enforced.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-known-signers-rust-as-codegen-enforced.json`](transcripts/raw/2026-06-18-2-known-signers-rust-as-codegen-enforced.json)
