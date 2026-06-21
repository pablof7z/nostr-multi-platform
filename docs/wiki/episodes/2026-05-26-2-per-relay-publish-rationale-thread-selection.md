---
type: episode-card
date: 2026-05-26
session: fbebb78b-07ed-4e26-8e2e-56fb66929a63
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/fbebb78b-07ed-4e26-8e2e-56fb66929a63.jsonl
salience: architecture
status: superseded
subjects:
  - outbox-resolver-trait
  - relay-selection-reason
  - publish-outbox-relay
supersedes: []
related_claims: []
source_lines:
  - 5854-5858
  - 5893-6109
  - 6112-6336
  - 6339-6614
  - 6620-6793
captured_at: 2026-06-18T05:45:51Z
---

# Episode: Per-relay publish rationale: thread selection reason from resolver to UI

## Prior State

OutboxResolver::resolve() returned BTreeSet<RelayUrl> with no rationale. The Nip65OutboxResolver made 5 distinct relay selection decisions (author write relays, local write fallback, discovery indexer, p-tag inbox fanout, explicit target) but discarded the 'why' at the trait boundary. InFlight stored only PerRelayState per relay. PublishOutboxRelay carried status/message but no reason. TUI's OutboxLine stripped relay data entirely.

## Trigger

User requested: 'see what's their publishing status for each relay we want to publish to — ideally we should be able to tell WHY we are publishing in that relay — i.e. because it p-tags pubkey x, because it's an app relay, because it's the current user's own relay, because we are republishing someone else's 10002 event we didn't find in an indexer relay, etc.'

## Decision

Thread relay selection rationale through the entire pipeline as a pre-formatted string owned by the resolver. Five layers: (1) OutboxResolver trait returns Vec<ResolvedRelay> (url + reason) instead of BTreeSet<RelayUrl>; (2) Nip65OutboxResolver annotates each code path with a human label; (3) InFlight gains parallel relay_reasons: BTreeMap<RelayUrl, String> (write-once, never mutated by retry); (4) PublishOutboxRelay gains relay_reason: String with serde(default, skip_serializing_if) for zero-cost backwards compat; (5) TUI gets OutboxRelayLine with reason field + detail pane UI. Any app gets rationale for free — just render the string.

## Consequences

- Every shell (iOS, Kotlin, TUI) can show 'why this relay' without understanding NIP-65, p-tag thresholds, or indexer logic
- Nip65OutboxResolver becomes the single point of truth for relay selection rationale
- Deduplication edge case: a relay appearing via both author-write AND indexer paths must pick one reason (first-wins)
- iOS change is two lines: add relayReason to Codable struct, render in OutboxRelayRow
- Test stubs StaticOutbox and NoopOutboxResolver return generic reasons trivially

## Open Tail

- Deduplication reason priority when same relay arrives from multiple code paths
- is_complete() eviction-on-any-Ok semantic deferred separately
- kind:10002 republication label explicitly out of scope

## Evidence

- transcript lines 5854-5858
- transcript lines 5893-6109
- transcript lines 6112-6336
- transcript lines 6339-6614
- transcript lines 6620-6793

