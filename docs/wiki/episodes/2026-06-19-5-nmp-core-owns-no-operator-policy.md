---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: active
subjects:
  - nmp-defaults
  - operator-relays
  - default-follows
  - nostrconnect-perms
supersedes:
  - 2026-06-19-6-operator-app-level-policy-extracted-from
related_claims: []
source_lines:
  - 33-34
  - 50-52
  - 1936-1941
  - 1978-1982
captured_at: 2026-06-19T11:51:39Z
---

# Episode: NMP core owns no operator policy defaults

## Prior State

DEFAULT_FOLLOWS (including hardcoded fiatjaf), operator relay lists, and nostrconnect sign_event:1,7 permissions were hardcoded inside nmp-core/nmp-defaults — NMP itself owned operator policy.

## Trigger

#1493 audit P9 finding + explicit user directive: "hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself."

## Decision

nmp-defaults reclassified as a reusable library owning no operator policy. Builder type-state forces explicit relay configuration. Operator relays, seed follows, and nostrconnect permissions moved to leaf apps (Chirp supplies from nmp-chirp-config). NMP owns no default policy.

## Consequences

- Breaking change for out-of-repo consumers (must supply own relays/follows/perms); migration notes in PR bodies
- In-repo consumers (chirp iOS/Android/tui/desktop, gallery, nmp-cli) upgraded in-PR
- 3 PRs merged: #1550 (relays/pubkeys/bootstrap out), #1581 (perms app-supplied)
- DEFAULT_FOLLOWS const removed; remaining grep hit is just an explanatory comment

## Open Tail

*(none)*

## Evidence

- transcript lines 33-34
- transcript lines 50-52
- transcript lines 1936-1941
- transcript lines 1978-1982

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-5-nmp-core-owns-no-operator-policy.json`](transcripts/2026-06-19-5-nmp-core-owns-no-operator-policy.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-5-nmp-core-owns-no-operator-policy.json`](transcripts/raw/2026-06-19-5-nmp-core-owns-no-operator-policy.json)
