---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - operator-policy
  - default-follows
  - nostrconnect-perms
  - nmp-defaults
supersedes:
  - 2026-06-19-3-nmp-core-removes-product-default-configuration
related_claims: []
source_lines:
  - 19-48
  - 50-52
  - 1937-1938
  - 1979-1982
captured_at: 2026-06-19T00:46:24Z
---

# Episode: Operator policy removed from NMP core; apps supply all defaults

## Prior State

Hardcoded DEFAULT_FOLLOWS (including fiatjaf), nostrconnect bootstrap relay, and sign_event:1,7 permissions were baked into nmp-core/nmp-defaults. nmp-defaults owned operator policy.

## Trigger

P9 finding in #1493 audit and user directive: 'hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself'.

## Decision

All operator policy moved to app level: seed follows, bootstrap relay, and signer permissions are app-supplied. nmp-defaults reclassified as a reusable lib owning no operator policy. Builder type-state forces explicit relay configuration. NIP-46 nostrconnect perms are app-supplied with no NMP default.

## Consequences

- nmp-core and nmp-defaults contain zero operator policy
- Leaf apps (Chirp, Gallery, etc.) must supply relays, follows, and perms explicitly via builder API
- DEFAULT_FOLLOWS const deleted (only an explanatory comment remains)
- Consumer-migration notes provided for out-of-repo consumers (podcast-player/hl)

## Open Tail

*(none)*

## Evidence

- transcript lines 19-48
- transcript lines 50-52
- transcript lines 1937-1938
- transcript lines 1979-1982

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-3-operator-policy-removed-from-nmp-core.json`](transcripts/2026-06-19-3-operator-policy-removed-from-nmp-core.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-3-operator-policy-removed-from-nmp-core.json`](transcripts/raw/2026-06-19-3-operator-policy-removed-from-nmp-core.json)
