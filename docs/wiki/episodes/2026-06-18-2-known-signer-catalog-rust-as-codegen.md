---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - known-signer-catalog
  - source-of-truth
  - codegen-enforcement
supersedes:
  - 2026-06-18-2-known-signers-rust-owned-single-source
related_claims: []
source_lines:
  - 27-28
  - 50-52
  - 1195-1208
  - 1240-1250
  - 1287-1298
  - 1430-1440
  - 1494-1505
captured_at: 2026-06-18T23:05:39Z
---

# Episode: Known-signer catalog: Rust as codegen-enforced sole writer

## Prior State

Known-signer lists were duplicated across Kotlin (×3 vendored copies in ExternalSignerWire.kt), Swift (×2 copies, already drifted on @MainActor), and web (NIP-07 detection only, not a catalog copy). No single source of truth. VendorDriftGate only checked byte-identity of Kotlin copies, not Swift. Labels had already drifted: nostrconnect labeled "Signer App" in some places and "Nostr Signer" in others; Amber was inconsistently named; nostrconnect was absent from some lists.

## Trigger

#1493 audit P4: "known signers" table duplicated + already drifted across native platforms. Agent investigation revealed the signer list is an embedded ~110-line section inside large triple-vendored multi-purpose files (ExternalSignerWire.kt), not standalone — making a monolithic codegen PR risky. Swift copies already differed on @MainActor; web is NIP-07-only, not a catalog copy at all.

## Decision

Rust (nmp-core signer_catalog.rs) is the sole writer of the known-signer catalog. Implementation was PHASED (not monolithic): PR2a established the Rust catalog + derived nip46_onboarding + converged native labels to match + added a Rust-tied `nmp gen signer-catalog --check` gate asserting native lists match catalog JSON. PR2b added mechanical codegen (nmp-codegen emitters generate Kotlin×3/Swift×2 lists + AndroidManifest <queries> + iOS plist from catalog, retire the hand-parse gate). codegen-drift.yml CI enforces native↔Rust parity.

## Consequences

- Label drift fixed: nostrconnect→"Nostr Connect", nostrsigner→"Amber", generic "Nostr Signer" row dropped
- nip46_onboarding now derives from the catalog (was previously hand-maintained separately)
- VendorDriftGate extended to cover generated Kotlin; old byte-identity parity replaced by native↔Rust structural check
- Swift @MainActor drift between two copies reconciled
- Known-signers vertical complete: PR1 (#1550, relays/pubkeys out) + PR2a (#1567) + PR2b (#1576) all merged
- Web/TS is NIP-07-only — not a codegen target, excluded from catalog

## Open Tail

- PR1b (nostrconnect perms in broker) still blocked on p5 #1547 landing (shared broker/nostrconnect.rs file)
- PR3 (signer-labels-to-shells + P4 F3) in progress as Direction A

## Evidence

- transcript lines 27-28
- transcript lines 50-52
- transcript lines 1195-1208
- transcript lines 1240-1250
- transcript lines 1287-1298
- transcript lines 1430-1440
- transcript lines 1494-1505

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-known-signer-catalog-rust-as-codegen.json`](transcripts/2026-06-18-2-known-signer-catalog-rust-as-codegen.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-known-signer-catalog-rust-as-codegen.json`](transcripts/raw/2026-06-18-2-known-signer-catalog-rust-as-codegen.json)
