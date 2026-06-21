---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - p9-hardcoded-config
  - nmp-core-identity
  - nmp-defaults
supersedes:
  - 2026-05-19-1-relay-source-of-truth-moved-from
related_claims: []
source_lines:
  - 19-48
  - 50-52
  - 154-155
captured_at: 2026-06-18T18:34:07Z
---

# Episode: P9 doctrine: operator relays/pubkeys must live in app layer only

## Prior State

Hardcoded operator relays, pubkeys, and DEFAULT_FOLLOWS (including fiatjaf) were embedded in generic/core layers (nmp-core, nmp-defaults, identity.rs), making them unreachable for app-level override and violating the principle that NMP core should be operator-neutral.

## Trigger

P9 finding in the 25-agent architecture audit (issue #1493) identified hardcoded operator relays/pubkeys in generic layers; user explicitly directed: 'hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself.'

## Decision

Operator-default relays, pubkeys, and follows are architecturally excluded from NMP core; they must reside solely in app-level code. This establishes a new system invariant: NMP crates must not contain operator-specific configuration.

## Consequences

- p9-core-config agent is sole owner of identity.rs and nmp-defaults to extract hardcoded values to app layer
- Known-signers Rust→native drift (P4) must also be resolved under this invariant — Rust is source of truth, native derives
- All identity labels and operator defaults move to app shell or configuration, not kernel crates

## Open Tail

- Exact extraction target (which app-layer module/config) still to be designed by agent with codex design-first pass
- Coordination may be needed with p4-native-policy lane if Rust kernel seams are required

## Evidence

- transcript lines 19-48
- transcript lines 50-52
- transcript lines 154-155

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-p9-doctrine-operator-relays-pubkeys-must.json`](transcripts/2026-06-18-1-p9-doctrine-operator-relays-pubkeys-must.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-p9-doctrine-operator-relays-pubkeys-must.json`](transcripts/raw/2026-06-18-1-p9-doctrine-operator-relays-pubkeys-must.json)
