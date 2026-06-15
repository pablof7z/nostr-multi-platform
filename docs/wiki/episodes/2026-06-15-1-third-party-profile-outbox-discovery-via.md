---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-kernel-profile-resolution
  - outbox-model
  - nip-65
supersedes: []
related_claims: []
source_lines:
  - 53-85
captured_at: 2026-06-15T04:48:56Z
---

# Episode: Third-party profile outbox discovery via kind:10002 D3 probe

## Prior State

The outbox router machinery for NIP-65 existed in the substrate but was inert for third-party profiles: kind:10002 was only fetched for the self/active account at startup. When the kernel needed a third-party kind:0, it queried only operator indexer relays (e.g. purplepag.es) via Lane 6, never the author's own NIP-65 write relays. Additionally, profile claims used multiple bespoke bypass paths (profile_claim_request, request_profile_for_rendered_note) with separate dedup/subscription logic, and probed_mailboxes was cleared on every relay connect regardless of whether the socket had actually gone down.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS; multi-agent investigation traced the root cause to the kernel never discovering third-party authors' NIP-65 relay lists, so kind:0 queries only went to indexer relays that only carry profiles someone explicitly pushed there.

## Decision

Unified all profile claims through a single registry chokepoint (register_profile_claim_interest → recompile_and_diff_with_lookup). On claim for an unknown pubkey, the kernel now issues a D3 probe: query kind:10002 on indexer relays to discover the author's write relays, then re-route the kind:0 subscription to those discovered relays (Nip65Arrived re-route). probed_mailboxes re-arm gated to genuine reconnects only (indexer_socket_was_down), not every connect event.

## Consequences

- Progressive profile resolution — never blank, resolution rate improved from ~10% baseline to ~50% with outbox discovery
- Single chokepoint eliminates duplicate subscriptions and race conditions from bespoke bypass paths
- drain_pending_reverify bug migrated to the same unified path
- Reconnect gating prevents redundant 10002 probes on flaky connections

## Open Tail

- Baseline still only ~50% — users who never published kind:10002 remain unresolvable via outbox; NIP-60 follow-list relay hints filed as follow-up (#1434) for a second discovery lane

## Evidence

- transcript lines 53-85
