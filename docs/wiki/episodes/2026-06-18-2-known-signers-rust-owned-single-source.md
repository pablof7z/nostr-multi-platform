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
  - vendordriftgate
  - nmp-codegen
supersedes:
  - 2026-06-18-2-known-signers-rust-catalog-as-single
related_claims: []
source_lines:
  - 1131-1146
  - 1223-1227
  - 1237-1250
  - 1287-1298
  - 1493-1505
captured_at: 2026-06-18T22:54:46Z
---

# Episode: Known-signers: Rust-owned single source of truth with codegen enforcement

## Prior State

Native KNOWN_NOSTR_SIGNERS lists were hand-authored and embedded as sections inside larger vendored files (ExternalSignerWire.kt ×3, NostrLoginBlock.swift ×2). VendorDriftGate checked native↔native byte-identity only — never against Rust. Labels had already drifted ("Signer App" vs "Nostr Signer" vs "Amber"; nostrconnect absent on Android).

## Trigger

Issue #1493 P4 audit found ≥6 definitions of is_replaceable with divergent answers, and known-signers tables duplicated and already drifted across Swift/Kotlin/web despite a Rust source of truth.

## Decision

Rust KnownSignerApp catalog is the single source of truth. Amber modeled as one NIP-55+NIP-46 entry; nostrconnect relabeled "Nostr Connect"; Android:None for entries that don't apply. nmp-codegen generates native Kotlin/Swift lists + AndroidManifest <queries> + iOS plist schemes from the catalog. codegen-drift.yml CI gate replaces the old native-only VendorDriftGate parity. Phased in two PRs: PR2a (catalog + label convergence + Rust-tied --check gate) then PR2b (full codegen emitters retiring hand lists). Both merged (#1567, #1576).

## Consequences

- Rust is now the enforced sole writer of signer data; codegen-drift CI catches any drift
- Hand-authored native signer lists deleted; VendorDriftGate extended to generated artifacts
- Future signer additions change one Rust struct — codegen propagates to all platforms
- Web (NIP-07 detection) is NOT a catalog copy and remains separate
- PR1b (nostrconnect permissions) deferred until p5 #1547 lands due to broker/nostrconnect.rs collision

## Open Tail

- PR1b (nostrconnect sign_event permissions) still gated on p5 #1547 merge
- PR3 (signer-labels-to-shells + P4 F3) in progress

## Evidence

- transcript lines 1131-1146
- transcript lines 1223-1227
- transcript lines 1237-1250
- transcript lines 1287-1298
- transcript lines 1493-1505

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-known-signers-rust-owned-single-source.json`](transcripts/2026-06-18-2-known-signers-rust-owned-single-source.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-known-signers-rust-owned-single-source.json`](transcripts/raw/2026-06-18-2-known-signers-rust-owned-single-source.json)
