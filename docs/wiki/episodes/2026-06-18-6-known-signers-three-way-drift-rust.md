---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - known-signers
  - source-of-truth
  - vendor-drift-gate
supersedes: []
related_claims: []
source_lines:
  - 265-280
captured_at: 2026-06-18T19:42:43Z
---

# Episode: Known-signers three-way drift: Rust, Kotlin, Swift tables already diverged with no cross-validation

## Prior State

Three independent known-signers tables existed: Rust `signer_apps_table()` (identity.rs), Android Kotlin ExternalSignerWire.kt (vendored byte-identical to cli registry), and iOS Swift NostrLoginBlock.swift. They had already diverged: nostrsigner is "Nostr Signer" in Rust but "Amber" in both native; nostrconnect is "Signer App" in Rust but "Nostr Signer" in Swift and absent on Android. VendorDriftGate only asserted native-to-native parity, never tying any back to Rust.

## Trigger

#1493 audit finding P9/P4; codex-design-first approved Rust-owned catalog + codegen.

## Decision

Rust-owned `KnownSignerApp` catalog struct (app_id, display_label, signer_kind, platform-specific capability metadata sub-structs). Ship as a runtime projection (rename nip46_onboarding → external_signers). Codegen native manifest/plist from the Rust catalog. Extend VendorDriftGate to assert native artifacts match the Rust catalog digest. Label call: "Amber" is the correct Rust-owned vendor label (not "Nostr Signer"); signer_kind for Amber should be "nip55" not "nip46".

## Consequences

- Single source of truth for known signers
- Drift between Rust and native tables becomes structurally impossible
- P4 Finding 3 (SignInScreen signerKind label) absorbed into P9 PR3 (signer labels to shells)

## Open Tail

- P9 PR2 (known-signers source-of-truth) queued after PR1 lands

## Evidence

- transcript lines 265-280

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-6-known-signers-three-way-drift-rust.json`](transcripts/2026-06-18-6-known-signers-three-way-drift-rust.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-6-known-signers-three-way-drift-rust.json`](transcripts/raw/2026-06-18-6-known-signers-three-way-drift-rust.json)
