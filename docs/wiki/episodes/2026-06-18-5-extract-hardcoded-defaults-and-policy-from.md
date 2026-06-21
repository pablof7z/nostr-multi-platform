---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - p9-core-config
  - default-follows
  - default-app-relays
  - known-signers
  - crate-boundaries
supersedes:
  - 2026-06-18-7-operator-policy-removed-from-nmp-core
related_claims: []
source_lines:
  - 19-48
  - 456-465
  - 549-555
captured_at: 2026-06-18T20:25:04Z
---

# Episode: Extract hardcoded defaults and policy from NMP core to app layer

## Prior State

DEFAULT_FOLLOWS (incl. fiatjaf), DEFAULT_APP_RELAYS, and bootstrap relay URLs were hardcoded in NMP core crates. Known-signers table was duplicated and already drifted across Swift/Kotlin/web despite a Rust source of truth. nmp-chirp-config role had also drifted.

## Trigger

Issue #1493 P9 and P4 Finding 3/6: hardcoded operator relays/pubkeys belong ONLY in app-level code, not in NMP itself.

## Decision

DEFAULT_FOLLOWS → initial_follows: Vec<String> param on ActorCommand::CreateAccount (parallel to relays, empty → no kind:3). DEFAULT_APP_RELAYS → builder type-state requiring explicit .with_relays(...) or .without_initial_relays(), no silent fallback. Bootstrap relay + perms → app-supplied, no product default. Known-signers → Rust-owned catalog + codegen'd native manifest/plist + VendorDriftGate tied to the Rust digest. P4 Finding 3 (signer label) and Finding 6 (nmp-chirp-config role drift) absorbed into p9 vertical. crate-boundaries.md §9 updated.

## Consequences

- Breaking API change: CreateAccount now requires explicit relay and follows parameters
- Three coupled breaking changes (relays, follows, perms) consolidated into one vertical to avoid half-compiled master states
- Known-signers drift will be mechanically prevented by VendorDriftGate digest comparison

## Open Tail

- PR1 (relays/pubkeys/perms out of NMP) still in progress; PR2 (known-signers source-of-truth) and PR3 (signer-labels-to-shells) follow sequentially
- p5 handshake crossbeam (#1547) must merge first due to broker/nostrconnect.rs overlap

## Evidence

- transcript lines 19-48
- transcript lines 456-465
- transcript lines 549-555

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-5-extract-hardcoded-defaults-and-policy-from.json`](transcripts/2026-06-18-5-extract-hardcoded-defaults-and-policy-from.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-5-extract-hardcoded-defaults-and-policy-from.json`](transcripts/raw/2026-06-18-5-extract-hardcoded-defaults-and-policy-from.json)
