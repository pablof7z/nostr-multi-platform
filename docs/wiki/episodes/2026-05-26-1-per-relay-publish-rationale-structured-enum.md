---
type: episode-card
date: 2026-05-26
session: 7174d4d4-371b-4b8e-87a6-91024c2b4c2a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7174d4d4-371b-4b8e-87a6-91024c2b4c2a.jsonl
salience: product
status: active
subjects:
  - resolved-relay
  - relay-selection-reason
  - publish-outbox
  - nip65-resolver
supersedes:
  - 2026-05-26-2-per-relay-publish-rationale-thread-selection
related_claims: []
source_lines:
  - 1-12
  - 155-160
  - 557-608
  - 934-940
  - 1002-1048
captured_at: 2026-06-18T05:57:29Z
---

# Episode: Per-relay publish rationale: structured enum replaces plain strings, kernel owns formatting

## Prior State

OutboxResolver::resolve() returned BTreeSet<RelayUrl> with no explanation for why each relay was targeted; users saw relays stuck in Pending with no context (the pending-reaction bug: p-tag NIP-65 fan-out adds author read relays, one fails to connect, reverts to Pending, and there was no way to explain that to the user). Apps had no data to render rationale.

## Trigger

PR #585 plan document specified end-to-end architecture for surfacing 'why each relay was targeted'; user directive to implement it. PR review later flagged that the core contract carried human-readable strings instead of structured data.

## Decision

Introduced ResolvedRelay struct carrying RelaySelectionReason enum variants (Nip65Write, AppRelay, DiscoveryIndexer, InboxRelay, ExplicitRelay) through the entire internal pipeline. All 5 Nip65OutboxResolver code paths annotate with variants. Engine deduplicates by canonical URL and merges distinct reasons. Kernel owns all display formatting via format_relay_reason() at the wire boundary — apps render pre-formatted strings verbatim, zero app-side logic. PublishQueueEntry.title pre-formatted by kernel replaces TUI's bespoke publish_kind_label(). iOS uses decodeIfPresent for relayReason, defaulting to empty string for forward-compat.

## Consequences

- No app-side logic for relay rationale (D5: backend computes, apps render verbatim)
- RelaySelectionReason enum keeps internal pipeline display-free; format_relay_reason() is the sole formatting site at the wire boundary
- PublishQueueEntry.title pre-formatted by kernel removes TUI kind-label duplication (RMP commandment #4)
- iOS forward-compatible with older kernels that omit relayReason
- Reason strings survive publish completion: InFlight.relay_reasons → TerminalOutcome.relay_reasons → RelayAckOutcome.relay_reason → publish_queue JSON

## Open Tail

- iOS OutboxRelayRow visual review of relayReason rendering in production

## Evidence

- transcript lines 1-12
- transcript lines 155-160
- transcript lines 557-608
- transcript lines 934-940
- transcript lines 1002-1048

