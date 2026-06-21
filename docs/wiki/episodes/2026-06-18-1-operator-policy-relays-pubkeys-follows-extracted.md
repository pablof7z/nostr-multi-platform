---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - p9-operator-policy
  - nmp-defaults
  - nmp-core-defaults
supersedes:
  - 2026-06-18-1-operator-relays-pubkeys-evacuated-from-nmp
related_claims: []
source_lines:
  - 50-52
  - 1047-1061
  - 1123-1129
captured_at: 2026-06-18T21:31:23Z
---

# Episode: Operator policy (relays/pubkeys/follows) extracted from NMP to leaf apps

## Prior State

DEFAULT_FOLLOWS (incl. fiatjaf seed follow) and DEFAULT_APP_RELAYS were hardcoded inside nmp-core and nmp-defaults — operator-specific policy baked into the platform-neutral layer.

## Trigger

#1493 P9 finding: hardcoded operator relays and pubkeys in generic NMP crates; owner directive that they belong ONLY in app-level code.

## Decision

Delete DEFAULT_FOLLOWS and DEFAULT_APP_RELAYS from nmp-core/nmp-defaults. nmp-defaults becomes a reusable library owning no operator policy (crate-boundaries §9 reclassified). Builder gains type-state (RelaysDeclared) forcing apps to explicitly declare relays via .with_relays()/.without_initial_relays(). Chirp gets nmp-chirp-config with CHIRP_DEFAULT_FOLLOWS injected through a typed wrapper.

## Consequences

- All consumers must now explicitly opt into relay configuration; bare nmp_app_create_new_account gets empty follows (no silent fiatjaf seed).
- nmp-chirp-config owns chirp-specific operator policy; other apps (podcast-player/hl) need their own wrapper or explicit empty.
- NIP-46 hardcoded perms (sign_event:1,7 in broker/nostrconnect.rs) split into separate PR1b — app-supplied perms, deferred after p5's handshake PR.

## Open Tail

- PR1b (nostrconnect perms as app-supplied) blocked on p5 #1547 landing first (shared file broker/nostrconnect.rs).

## Evidence

- transcript lines 50-52
- transcript lines 1047-1061
- transcript lines 1123-1129

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-operator-policy-relays-pubkeys-follows-extracted.json`](transcripts/2026-06-18-1-operator-policy-relays-pubkeys-follows-extracted.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-operator-policy-relays-pubkeys-follows-extracted.json`](transcripts/raw/2026-06-18-1-operator-policy-relays-pubkeys-follows-extracted.json)
