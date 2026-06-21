---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - known-signers-catalog
  - native-signer-drift
  - codegen-enforcement
supersedes:
  - 2026-06-18-2-known-signer-catalog-rust-as-codegen
related_claims: []
source_lines:
  - 27-28
  - 1217-1256
  - 1287-1298
  - 1430-1438
  - 1494-1505
captured_at: 2026-06-18T23:24:57Z
---

# Episode: Known-signers: Rust becomes codegen-enforced sole source of truth

## Prior State

Native code (Kotlin ×3 copies, Swift ×2 copies) held known-signer tables independently with no Rust enforcement. VendorDriftGate only checked Kotlin-native parity. Swift copies had already diverged from each other (one missing @MainActor). Labels had drifted ("Nostr Signer" vs "Nostr Connect", missing Amber/nostrconnect entries). Web used NIP-07 window detection, not a catalog copy.

## Trigger

#1493 P4 audit found known-signers duplicated across platforms with real drift (Amber/"Nostr Connect"/nostrconnect absent or mislabeled) and no enforcement mechanism linking native copies to the Rust source of truth.

## Decision

nmp-core::signer_catalog is the sole source of truth. Two-phase delivery: PR2a creates the Rust catalog + derives nip46_onboarding + converges native label drift + adds a Rust-tied --check gate (native↔Rust parity); PR2b adds nmp-codegen emitters that generate Kotlin/Swift lists + AndroidManifest <queries> + iOS plist schemes from the catalog, retiring hand-maintained native lists and the PR2a hand-parse gate. codegen-drift CI enforces parity going forward.

## Consequences

- Amber unified as single nostrsigner entry with caps [Nip55, Nip46] on both platforms; generic "Nostr Signer" row dropped
- "Nostr Connect" label standardized (was "Nostr Signer"/"Signer App" across copies)
- VendorDriftGate extended to generated Kotlin; PR2a hand-parse gate retired by PR2b codegen
- Web remains NIP-07 detection only — not a codegen target (no app list)
- Swift @MainActor drift between copies reconciled in PR2a
- PHASED delivery chosen over FULL monolithic PR due to embedded sections in triple-vendored multi-purpose files
- CI blind spot: apps/* tests not compiled by CI (#1553); local cargo test -p nmp-app-chirp required before every push

## Open Tail

- PR1b (nostrconnect permissions) still gated on p5 #1547 merging (shared broker/nostrconnect.rs)

## Evidence

- transcript lines 27-28
- transcript lines 1217-1256
- transcript lines 1287-1298
- transcript lines 1430-1438
- transcript lines 1494-1505

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-known-signers-rust-becomes-codegen-enforced.json`](transcripts/2026-06-18-2-known-signers-rust-becomes-codegen-enforced.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-known-signers-rust-becomes-codegen-enforced.json`](transcripts/raw/2026-06-18-2-known-signers-rust-becomes-codegen-enforced.json)
