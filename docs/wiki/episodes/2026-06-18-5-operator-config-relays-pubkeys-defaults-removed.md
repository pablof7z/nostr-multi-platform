---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-defaults
  - operator-config
  - crate-boundaries
supersedes:
  - 2026-05-19-1-seed-new-accounts-with-default-follows
  - 2026-06-18-1-operator-config-is-app-level-only
related_claims: []
source_lines:
  - 260-293
  - 457-465
captured_at: 2026-06-18T19:42:43Z
---

# Episode: Operator config (relays, pubkeys, defaults) removed from nmp-core to app layer

## Prior State

DEFAULT_FOLLOWS (2 hex pubkeys incl. fiatjaf), DEFAULT_APP_RELAYS, nostrconnect bootstrap relay (wss://relay.damus.io), and nostrconnect permissions were hardcoded inside nmp-core. nmp-defaults was effectively acting as a leaf app rather than a composition library.

## Trigger

#1493 audit finding P9; codex-design-first approved the full vertical.

## Decision

All operator/app policy removed from nmp-core: (a) DEFAULT_FOLLOWS → `initial_follows: Vec<String>` param on ActorCommand::CreateAccount (empty → no kind:3 published); (b) DEFAULT_APP_RELAYS → builder type-state requires `.with_relays(...)` or explicit `.without_initial_relays()`, no silent fallback; (c) bootstrap relay → `NostrConnectBootstrap::Relay|Disabled`, fail-observable if unset; (d) nostrconnect permissions → app-supplied `PermissionRequest` with `Nip46Permission::sign_event(kind)` helpers, no product default. Update crate-boundaries.md §9 to state nmp-defaults is a composition library, NOT a leaf app, and must not own operator policy.

## Consequences

- P9 lane granted full vertical ownership (option A) to avoid half-compiling master
- P4 Finding 6 (chirpConfig.ts relay role drift: Rust says "both", TS says "both,indexer") absorbed into P9 PR1
- No silent fallback to hardcoded operator data — app must explicitly opt in or out
- crate-boundaries.md §9 updated as design constraint

## Open Tail

- P9 PR1 (relays/pubkeys/bootstrap/perms) in progress; PR2 (known-signers) and PR3 (signer labels) queued

## Evidence

- transcript lines 260-293
- transcript lines 457-465

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-5-operator-config-relays-pubkeys-defaults-removed.json`](transcripts/2026-06-18-5-operator-config-relays-pubkeys-defaults-removed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-5-operator-config-relays-pubkeys-defaults-removed.json`](transcripts/raw/2026-06-18-5-operator-config-relays-pubkeys-defaults-removed.json)
