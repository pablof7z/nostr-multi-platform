---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-defaults
  - nmp-core
  - nmp-chirp-config
supersedes:
  - 2026-06-18-5-operator-config-relays-pubkeys-defaults-removed
related_claims: []
source_lines:
  - 275-291
  - 457-458
  - 552-554
captured_at: 2026-06-18T20:12:30Z
---

# Episode: Operator policy removed from NMP core (D0/D4 doctrine)

## Prior State

DEFAULT_FOLLOWS (including hardcoded fiatjaf follow), DEFAULT_APP_RELAYS, bootstrap relay URLs, and signer permissions were embedded in NMP core/generic layers. Known-signers table was duplicated across Swift/Kotlin/web with already-drifted content. nmp-defaults was treated as a leaf app rather than a composition library.

## Trigger

Issue #1493 audit (P9/P4) identified operator policy in core as D0/D4 violations. Codex exec design review produced specific replacement patterns. P4 F6 confirmed chirpConfig.ts has already diverged from Rust nmp-chirp-config (content-relay role: Rust='both' vs TS='both,indexer').

## Decision

DEFAULT_FOLLOWS → initial_follows: Vec<String> param on ActorCommand::CreateAccount (empty = no kind:3 event). DEFAULT_APP_RELAYS → builder type-state requiring .with_relays() or .without_initial_relays(); no silent fallback. Bootstrap relay → NostrConnectBootstrap::Relay|Disabled, fail-observable if unset. Permissions → app-supplied PermissionRequest; protocol provides Nip46Permission::sign_event(kind) helpers, no product default. nmp-defaults is a reusable composition library, NOT a leaf app; must not own operator policy. Known-signers → Rust-owned catalog + codegen'd native manifest/plist + VendorDriftGate tied to Rust digest. Signer labels → shipped on signer_state projection, rendered verbatim by shells.

## Consequences

- No NMP core crate can embed operator-specific relay URLs, seed pubkeys, or auto-follow lists.
- crate-boundaries.md §9 updated to codify this.
- chirpConfig.ts role drift ('both' vs 'both,indexer') will be resolved by generating TS/JSON from Rust nmp-chirp-config (single source of truth).
- Known-signers drift gate prevents future Swift/Kotlin/web divergence.
- Signer labels (Amber/nsec/npsec/nip55) move from Rust display helpers to projection-owned semantic tokens rendered by shells.

## Open Tail

- P9 PR1 (relays/pubkeys/perms vertical) is in progress; PR2 (known-signers) and PR3 (signer-labels + F3/F6) queued.
- Web ProjectionMergeCache→wasm (P4 F5) and web config single-source (P4 F6) deferred as post-v1 follow-up issues.

## Evidence

- transcript lines 275-291
- transcript lines 457-458
- transcript lines 552-554

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-7-operator-policy-removed-from-nmp-core.json`](transcripts/2026-06-18-7-operator-policy-removed-from-nmp-core.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-7-operator-policy-removed-from-nmp-core.json`](transcripts/raw/2026-06-18-7-operator-policy-removed-from-nmp-core.json)
