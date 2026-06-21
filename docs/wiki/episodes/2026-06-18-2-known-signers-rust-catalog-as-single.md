---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - known-signers
  - nmp-core
  - vendor-drift-gate
  - nmp-codegen
supersedes:
  - 2026-06-18-6-known-signers-three-way-drift-rust
related_claims: []
source_lines:
  - 1129-1146
  - 1195-1208
  - 1223-1226
captured_at: 2026-06-18T21:02:14Z
---

# Episode: Known-signers: Rust catalog as single source-of-truth with codegen

## Prior State

Native signers lists were hardcoded separately in Kotlin (KNOWN_NOSTR_SIGNERS in ExternalSignerWire.kt), Swift (knownSigners in NostrLoginBlock.swift), and TS. VendorDriftGate enforced byte-identical native↔native copies but NEVER tied to Rust. Labels had drifted: Rust said 'Nostr Signer', native said 'Amber'; nostrconnect was absent on Android. The signer catalog and nip46_onboarding projection were also disconnected sources.

## Trigger

P4/P9 audit finding: known-signers tables duplicated and already drifted across Swift/Kotlin/web despite a Rust source of truth. Codex review confirmed one Rust catalog must drive both surfaces (detection lists + nip46_onboarding projection).

## Decision

Create a single Rust KnownSignerApp catalog ({app_id, display_label, capabilities:[Nip55|Nip46], android:Option<AndroidSpec>, ios:Option<IosSpec>}) as the authoritative source. signer_apps_table()/nip46_onboarding now DERIVE from it. Amber correctly modeled as a NIP-55 Android-only entry behind nostrsigner://; generic NIP-46 nostrsigner and nostrconnect are separate entries; nostrconnect label fixed to 'Nostr Connect'. Implementation phased: PR2a (catalog + label convergence + Rust-tied --check gate replacing native-only VendorDriftGate) then PR2b (full codegen emitters replacing hand-authored native lists + manifest/plist generation).

## Consequences

- Single Rust source eliminates future native↔Rust drift
- VendorDriftGate will be reworked from native↔native to native↔Rust catalog --check
- ExternalSignerWire.kt must be split: KNOWN_NOSTR_SIGNERS section extracted to generated sibling
- nmp-codegen gains signer-catalog emitters (Kotlin/Swift/TS lists + AndroidManifest <queries> + iOS plist schemes)

## Open Tail

- PR2a in progress (catalog committed), PR2b (codegen) to follow immediately

## Evidence

- transcript lines 1129-1146
- transcript lines 1195-1208
- transcript lines 1223-1226

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-known-signers-rust-catalog-as-single.json`](transcripts/2026-06-18-2-known-signers-rust-catalog-as-single.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-known-signers-rust-catalog-as-single.json`](transcripts/raw/2026-06-18-2-known-signers-rust-catalog-as-single.json)
