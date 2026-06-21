---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - operator-config
  - nmp-defaults
  - default-follows
  - nostrconnect-perms
supersedes:
  - 2026-06-19-3-operator-policy-removed-from-nmp-core
related_claims: []
source_lines:
  - 50-51
  - 1937-1941
  - 1964-1966
captured_at: 2026-06-19T06:25:53Z
---

# Episode: Operator config (relays, seed follows, nostrconnect perms) removed from NMP core to app layer

## Prior State

DEFAULT_FOLLOWS (incl. fiatjaf), hardcoded operator relays, and nostrconnect sign_event:1,7 permissions were embedded in nmp-core/nmp-defaults — generic crates shipping product-level operator policy.

## Trigger

P9 audit finding (hardcoded operator relays/pubkeys); user directive: 'hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself'

## Decision

nmp-defaults reclassified as a reusable lib owning NO operator policy. Builder type-state forces explicit relays (.with_relays / .without_initial_relays). nostrconnect permissions are app-supplied via a pre-start config slot; NMP owns no default. Leaf apps (Chirp) supply config from nmp-chirp-config.

## Consequences

- DEFAULT_FOLLOWS const removed from identity.rs (only an explanatory comment remains)
- nmp-defaults crate boundary reclassified per §9 — owns no operator policy
- nostrconnect broker no longer hardcodes sign_event:1,7; apps must supply permissions
- Breaking change for out-of-repo consumers (podcast-player/hl) — migration notes in PR bodies
- In-repo consumers (chirp iOS/Android/tui/desktop, gallery, nmp-cli) upgraded in-PR

## Open Tail

*(none)*

## Evidence

- transcript lines 50-51
- transcript lines 1937-1941
- transcript lines 1964-1966

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-2-operator-config-relays-seed-follows-nostrconnect.json`](transcripts/2026-06-19-2-operator-config-relays-seed-follows-nostrconnect.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-2-operator-config-relays-seed-follows-nostrconnect.json`](transcripts/raw/2026-06-19-2-operator-config-relays-seed-follows-nostrconnect.json)
