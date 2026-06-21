---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - default-follows
  - operator-relays
  - nostrconnect-perms
  - nmp-defaults
  - nmp-core
supersedes:
  - 2026-06-19-2-operator-config-relays-seed-follows-nostrconnect
related_claims: []
source_lines:
  - 33-34
  - 1937-1941
  - 1978-1982
captured_at: 2026-06-19T11:35:40Z
---

# Episode: Operator/app-level policy extracted from NMP core into leaf apps

## Prior State

nmp-core and nmp-defaults contained hardcoded operator relays, seed follows (including fiatjaf as DEFAULT_FOLLOWS), nostrconnect bootstrap relays, and NIP-46 sign_event:1,7 permissions. nmp-defaults acted as a policy-owning crate.

## Trigger

#1493 audit P9 flagged hardcoded operator relays/pubkeys in generic layers; user explicitly stated: "hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself" and listed P9 as a critical item requiring codex design review.

## Decision

DEFAULT_FOLLOWS const removed from nmp-core (now just an explanatory comment). Operator relays, seed follows, and nostrconnect bootstrap relays moved to leaf apps via builder type-state pattern (apps must explicitly .with_relays() or .without_initial_relays()). NIP-46 nostrconnect:// sign_event:1,7 permissions are now app-supplied (Chirp supplies from nmp-chirp-config; NMP owns no default). nmp-defaults reclassified as a reusable library owning no operator policy.

## Consequences

- NMP core is now policy-free — any app using it must explicitly supply its own relays, follows, and signer permissions
- Builder type-state forces explicit relay configuration at compile time (no silent default)
- Consumer-migration notes in PR bodies for out-of-repo git-rev consumers (podcast-player/hl)
- In-repo consumers (chirp iOS/Android/tui/desktop, gallery, nmp-cli) upgraded in-PR

## Open Tail

*(none)*

## Evidence

- transcript lines 33-34
- transcript lines 1937-1941
- transcript lines 1978-1982

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-6-operator-app-level-policy-extracted-from.json`](transcripts/2026-06-19-6-operator-app-level-policy-extracted-from.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-6-operator-app-level-policy-extracted-from.json`](transcripts/raw/2026-06-19-6-operator-app-level-policy-extracted-from.json)
