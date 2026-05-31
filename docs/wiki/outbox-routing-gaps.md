---
title: Outbox Routing Gaps (D3)
slug: outbox-routing-gaps
summary: The outbox routing (D3 doctrine) is not wired end-to-end
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-27
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:7f0f0c78-d1aa-49db-b659-c9cf49827117
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# Outbox Routing Gaps (D3)

## Outbox Routing Gaps

The outbox routing (D3 doctrine) is not wired end-to-end. SubscriptionCompiler and Lifecycle ship, but kernel/requests/{profile,thread}.rs still emit REQs to hardcoded relay.primal.net and purplepag.es. The Nip65OutboxResolver adds each p-tag author's kind:10002 read relays to the publish target set, causing reactions to fan out to the reacted-to author's inbox relays. When mark_relay_unavailable() is called, any InFlight state for that relay reverts to Pending in the publish engine. A publish event whose per-relay state includes at least one Ok relay must not display as 'pending'; the publish_outbox_status function must check for Ok before Pending. The OutboxResolver trait must return a Vec<ResolvedRelay> (containing both url and reason) instead of a BTreeSet<RelayUrl>. Each relay selection code path in Nip65OutboxResolver must annotate the relay with a human-readable reason string: 'NIP-65 write relay', 'App relay (local config)', 'Discovery indexer (kind {n})', 'Inbox relay for {short_npub(recipient)}', or 'Explicit relay'. The InFlight struct must store a write-once relay_reasons BTreeMap alongside per_relay, mapping each relay URL to its selection reason string. The PublishOutboxRelay kernel type must include a relay_reason field with serde attributes default and skip_serializing_if = 'String::is_empty' for backwards compatibility. The relay_reason field on PublishOutboxRelay must be a pre-formatted human-readable string owned by the kernel, so that any app (iOS, Android, web, CLI) can render it without understanding NIP-65, p-tag fanout, or indexer role logic. When a relay appears via multiple resolver code paths (e.g., both author write and indexer), the first-assigned reason must be preserved (deduplication keeps the existing entry's reason). V-75 tracks that Lane 7 catch-all is silent, preventing routing-trace from attributing empty-outbox causes.

<!-- citations: [^7f0f0-13] [^fbebb-3] [^cd2b6-20] -->
## See Also

The backend changes (ResolvedRelay struct, trait change, Nip65OutboxResolver annotation, InFlight.relay_reasons, PublishOutboxRelay.relay_reason) must be a single PR with zero shell-level changes. The per-relay publish status with rationale feature must be documented and tracked as PR #585.

<!-- citations: [^fbebb-4] -->
