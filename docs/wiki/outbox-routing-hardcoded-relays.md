---
title: Outbox Routing End-to-End — Hardcoded Relay Constants Gap
slug: outbox-routing-hardcoded-relays
summary: Outbox routing is not wired end-to-end
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
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:bbafe8a2-8814-4625-83b6-6af3d4ec0412
  - session:fc128f85-af57-41cd-8c5b-a71d15450e17
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
  - session:7174d4d4-371b-4b8e-87a6-91024c2b4c2a
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# Outbox Routing End-to-End — Hardcoded Relay Constants Gap

## Hardcoded Relay Constants

No relay URLs or pubkeys are hardcoded in production Rust code; all such constants are gated behind #[cfg(test)]. Relay URLs are provided by the app side, including the indexer relay. Kernel::bootstrap_urls_for_role(role) reads from the app-provided relay_edit_rows config and returns URLs matching the role, returning empty if nothing is configured. Kernel::bootstrap_discovery_relays() returns the union of indexer and content URLs from relay_edit_rows. spawn_missing_relays and all production call sites (outbox, requests, status, lifecycle) read relay URLs from kernel config instead of constants. On the Swift side, KernelModel seeds wss://r.f7z.io (role: both) and wss://purplepag.es (role: indexer) before kernel.start() if relayEditRows is empty; r.f7z.io must not be used for writes. MarmotBridge.swift no longer hardcodes the [damus, nos.lol, primal] array; it reads from the app's relay config. relay.primal.net is the correct public relay for publishing and subscribing. The indexer relay set must never receive kind:1 content REQs; authors without a known kind:10002 mailbox are not queried for content anywhere. The indexer relay may serve content REQs only if an author independently declares it in their own kind:10002 relay list. The indexer set should be user-configurable rather than hardcoded. Outbox/relay routing is not hardcoded in nmp-core — it is a trait (OutboxRouter) that nmp-core injects, with implementations provided by a separate crate so competing routing algorithms can be swapped in. Kind 10006 (blocked relay list) is enforced in outbox routing to prevent WebSocket connections to blocked relays.

BlockedRelaySet infrastructure exists in routing.rs and the router checks ctx.blocked_relays.contains(url) on every lane, but build_routing_context() was instantiating it as empty.

All 4 build_routing_context() call sites in mailboxes.rs now read from BlockedRelayLookup instead of constructing an empty BlockedRelaySet.

FALLBACK_CONTENT_RELAY and FALLBACK_INDEXER_RELAY activate silently when relay rows are empty, causing users to publish to unconsented relays.

<!-- citations: [^64f3e-7] [^64f3e-8] [^7f0f0-8] [^582fc-11] [^bbafe-2] [^fc128-6] [^fd809-8] [^fe79b-12] [^1670f-15] [^64f3e-6] [^cd2b6-7] -->
## Relay Selection Algorithm

The generic relay selection algorithm uses a greedy max-coverage approach (applesauce-style) with per-author caps and a global connection budget, not naive fanout to every declared write relay. The apply_selection greedy algorithm uses deterministic tiebreak without NDK's connected-relay preference to avoid feedback churn. The outbox production recompile path is: compile → strip dead relays → apply_selection(greedy max-coverage, max_connections=30, max_per_user=2) → coverage hook → watermark rewrite → wire-emitter diff. Relays known to be dead are excluded from the candidate set before apply_selection runs, via a dead_relays state fed by repeated connection failures. A RelayHealthChanged trigger exists in the kernel so the actor can mark relays dead or alive, feeding back into recompile_and_diff. A personal-relay URL filter (bech32/hex pubkey in path, personal query params) is not needed because the greedy selector naturally discards coverage=1 relays. There is no RoutingRule registry; the router is a single closed generic algorithm. NIP crates that already know their relay set pass it via RoutingContext::explicit_targets, and the generic algorithm is skipped entirely.

<!-- citations: [^bbafe-3] [^1670f-16] -->
## REQ Routing Rules

REQ with no authors and no #p tag routes to the logged-in user's NIP-65 read relays union app relays. Each relay worker sends its REQ the moment its WebSocket handshake completes, without waiting for other relays to connect. [^bbafe-4]

## Publish Routing Rules

Per-kind relay routing is a dispatch table: kind:1 → kind:10002 write relays, kind:14/1059 → recipient's kind:10050 inbox, NIP-29 group events → relay from h tag, Marmot → group relay, drafts → private storage relay — NIP-65 (kind:10002) is just the default, not the only strategy. Plain publish routes to the author's NIP-65 write relays union app relays. Nip65OutboxResolver.resolve() adds each p-tag author's kind:10002 read relays to the publish target set, which causes reactions to target the event author's inbox relays in addition to the author's own write relays. App relays are additive to NIP-65 writes even when NIP-65 is known, not just a cold-start substitute. Writes where an author has no NIP-65 mailbox fall back to app relays if any exist, otherwise fail with a kernel-observable diagnostic surfaced to the app. Reads where an author has no NIP-65 mailbox fall back to app relays if any exist, otherwise fail with a kernel-observable diagnostic surfaced to the app. When an author has no app relays and no NIP-65 relays, that author is simply unroutable and the app must handle it. CompiledPlan exposes unroutable_authors as a BTreeSet<Pubkey> diagnostic so the kernel/app can surface authors that have neither NIP-65 nor app relays.

RelaySelectionReason is an enum used throughout the internal pipeline; human-readable formatting is isolated to a single site (format_relay_reason() in publish_outbox.rs) rather than embedding strings in the core contract. Outbox UIs surface per-relay reasons (e.g. 'NIP-65 write relay', 'Inbox relay for npub1abc…') alongside per-relay publish status. relay_reasons are threaded through TerminalOutcome → RelayAckOutcome so that durable history rows in publish_queue also carry the 'why' string.

mark_relay_unavailable() reverts InFlight per-relay states back to Pending, so a relay that fails to connect after dispatch causes its state to regress from InFlight to Pending. When a relay accepts a published event (Ok state) but other target relays are still Pending, the outbox status must display 'queued' rather than 'pending'. publish_outbox_status() must not return 'pending' when at least one relay has accepted the event (Ok state); the publish status function must check for any Ok relay state before checking for Pending states. InFlight must include a relay_reasons: BTreeMap<RelayUrl, String> field that is write-once at publish time and never mutated by retry logic. PublishOutboxRelay must include a relay_reason: String field with serde attributes #[serde(default, skip_serializing_if = "String::is_empty")] for zero-cost backwards compatibility. publish_outbox_relay() must accept a reason string from InFlight.relay_reasons and include it in the projected PublishOutboxRelay. iOS PublishOutboxRelay decoding uses a custom init(from:) with decodeIfPresent to default missing relay_reason to an empty string, preventing decode failures on older kernels. is_complete() semantics (require all relays terminal for outbox eviction) and global outbox access from any tab are explicitly out of scope for this change.

<!-- citations: [^fbebb-5] [^bbafe-5] [^1670f-17] [^fbebb-4] [^7174d-3] -->
## DM Routing Rules

DM (NIP-17 gift-wrapped) relay routing remains fail-closed: only the resolved recipient inbox is used, no app relays added, no fallback. [^bbafe-6]

## Mailbox Discovery

The kernel implicitly auto-emits kind:10002 discovery REQs for any author targeted by a REQ when that author's mailbox is neither cached nor previously probed. MAILBOX_PROBE_BATCH is set to 500 authors per discovery REQ. Implicit kind:10002 discovery probe REQs are session-sticky: an author probed once (even with an empty EOSE) is never re-probed within the session, with clear_probed_mailboxes as the refresh escape hatch. ingest_relay_list fires CompileTrigger::Nip65Arrived on every kind:10002 that replaces a mailbox, so a successful discovery triggers a recompile that routes the author via NIP-65. [^bbafe-7]

## Testing

The nmp-repl drives the real SubscriptionLifecycle (not a manual reimplementation) so it exercises implicit discovery, Nip65Arrived round-trip, apply_selection, and the dead-relay filter. [^bbafe-8]

## TUI Outbox Pane

The TUI Settings outbox pane renders a read-only history section of all past publishes (publish_queue), not just active/in-flight events. The published history section is displayed newest-first, capped at 20 items (kernel caps at 16). The kernel provides a pre-formatted PublishQueueEntry.title that the TUI reads verbatim, eliminating TUI duplication of kernel display logic for publish kind labels. [^7174d-4]
## See Also

