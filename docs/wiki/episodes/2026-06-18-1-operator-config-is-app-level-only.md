---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-defaults
  - known-signers
  - hardcoded-relays
  - core-config-locality
supersedes:
  - 2026-06-18-1-p9-doctrine-operator-relays-pubkeys-must
related_claims: []
source_lines:
  - 19-48
  - 50-51
  - 129-131
  - 143-165
captured_at: 2026-06-18T19:28:13Z
---

# Episode: Operator config is app-level only, not NMP core

## Prior State

Hardcoded operator relays, pubkeys (including DEFAULT_FOLLOWS referencing fiatjaf), and known-signers tables were embedded inside NMP core/generic layers, with native copies already drifted from the Rust source of truth.

## Trigger

25-agent architecture audit (issue #1493) identified P9 — hardcoded operator relays/pubkeys in generic layers; user explicitly directed: "P9 (hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself)"

## Decision

Operator configuration (relays, pubkeys, default follows, known-signers) must reside exclusively in app-level code; NMP core must not contain operator-specific or environment-specific config values.

## Consequences

- P9 agent dispatched to extract all hardcoded config from core to app-level shells
- Known-signers Rust↔native drift must be resolved by making the native shells the authoritative location, not duplicating from Rust
- Future doctrine: any operator-specific config introduced in generic layers is an architectural violation
- p9-core-config agent owns identity.rs as sole owner to prevent cross-lane conflicts

## Open Tail

- PR #1525 (snapshot-projector removal) overlaps P1 presentation-extraction work and may need coordination with config-extraction changes
- Known-signers unification between Rust and native shells not yet resolved — p4-native-policy agent owns native side but may need a Rust kernel seam from p9

## Evidence

- transcript lines 19-48
- transcript lines 50-51
- transcript lines 129-131
- transcript lines 143-165

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-operator-config-is-app-level-only.json`](transcripts/2026-06-18-1-operator-config-is-app-level-only.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-operator-config-is-app-level-only.json`](transcripts/raw/2026-06-18-1-operator-config-is-app-level-only.json)
