---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - hardcoded-config
  - product-defaults
  - nostrconnect-permissions
  - operator-relays
supersedes:
  - 2026-06-18-3-hardcoded-operator-relays-pubkeys-removed-from
related_claims: []
source_lines:
  - 50-52
  - 1854-1855
captured_at: 2026-06-19T00:18:35Z
---

# Episode: NMP core removes product-default configuration

## Prior State

Hardcoded operator relays/pubkeys (incl. DEFAULT_FOLLOWS with fiatjaf) lived in generic NMP layers. Hardcoded sign_event:1,7 permissions were embedded in broker/nostrconnect.rs.

## Trigger

#1493 P9 audit found hardcoded config in generic layers; user directive at line 50: 'hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself.'

## Decision

Operator relays/pubkeys moved to app-level code. nostrconnect permissions become app-supplied via a pre-start config slot (default empty in NMP; Chirp supplies from nmp-chirp-config). NMP core is product-agnostic.

## Consequences

- NMP core no longer embeds any product-default relay lists, follow sets, or signer permissions
- Apps must explicitly supply relay lists and signer permissions
- Nip17DmRelay added to relay_bypasses_selection — no more silent DM inbox relay pruning (P7 correctness fix)
- NostrConnect register.rs at 499 LOC (trimmed from 511 to meet file-size ceiling)

## Open Tail

*(none)*

## Evidence

- transcript lines 50-52
- transcript lines 1854-1855

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-3-nmp-core-removes-product-default-configuration.json`](transcripts/2026-06-19-3-nmp-core-removes-product-default-configuration.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-3-nmp-core-removes-product-default-configuration.json`](transcripts/raw/2026-06-19-3-nmp-core-removes-product-default-configuration.json)
