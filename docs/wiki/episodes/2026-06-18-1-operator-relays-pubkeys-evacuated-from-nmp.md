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
  - operator-policy
supersedes:
  - 2026-06-18-5-extract-hardcoded-defaults-and-policy-from
related_claims: []
source_lines:
  - 50-51
  - 1046-1061
  - 1123-1128
captured_at: 2026-06-18T21:02:14Z
---

# Episode: Operator relays/pubkeys evacuated from NMP core to app-level

## Prior State

DEFAULT_FOLLOWS (incl. fiatjaf) and DEFAULT_APP_RELAYS were hardcoded inside nmp-core and nmp-defaults, making the generic library carry operator-specific policy. Any consumer of nmp-defaults got relay/follow policy whether they wanted it or not.

## Trigger

P9 audit finding flagged hardcoded operator relays and pubkeys in generic layers; user directive (line 50): 'hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself.'

## Decision

Removed DEFAULT_FOLLOWS and DEFAULT_APP_RELAYS from NMP entirely. Created nmp-chirp-config as the app-level config crate owning CHIRP_DEFAULT_FOLLOWS. nmp-defaults reclassified (crate-boundaries §9) as a reusable library owning no operator policy. Added type-state builder pattern (RelaysDeclared) forcing consumers to explicitly declare relays via .with_relays()/.without_initial_relays(). ActorCommand::CreateAccount gains initial_follows: Vec<String> (empty → no kind:3). New FFI helper create_new_account_with_initial_follows for Chirp.

## Consequences

- Breaking change: any consumer not using the Chirp wrapper must now explicitly provide relays/follows or opt out
- nmp-defaults can no longer be used as a drop-in with implicit operator policy
- nmp-chirp-config becomes the single place for Chirp-specific operator config
- F6 config drift (chirpConfig.ts 'both,indexer' vs Rust 'both') fixed

## Open Tail

- Podcast-player/hl consumer migration needed (noted in PR body)

## Evidence

- transcript lines 50-51
- transcript lines 1046-1061
- transcript lines 1123-1128

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-operator-relays-pubkeys-evacuated-from-nmp.json`](transcripts/2026-06-18-1-operator-relays-pubkeys-evacuated-from-nmp.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-operator-relays-pubkeys-evacuated-from-nmp.json`](transcripts/raw/2026-06-18-1-operator-relays-pubkeys-evacuated-from-nmp.json)
