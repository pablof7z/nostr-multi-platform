---
title: Chirp Diagnostics & Relay Detail View
slug: chirp-diagnostics-view
summary: Tapping a relay row in the Diagnostics view navigates to a RelayDetailView showing full relay status (connection, auth, bytes RX/TX, timestamps, last notice, la
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:982efe9c-8edb-4ac6-b82a-aa5fef120c9a
  - session:42252c03-76ca-449c-9cfd-ed5949b2bb9d
  - session:5d893073-9635-450b-b8e9-50648bc1a4e7
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
  - session:594b7c34-efd1-4461-81ad-9fa33a6e76f9
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
---

# Chirp Diagnostics & Relay Detail View

## Relay Detail Navigation

The diagnostics navigation flow is Diagnostics → Relays → [select relay] → subscriptions, connectivity stats, notices → [select subscription] → event count, filters, timing. The diagnostics view displays all connected relays, including per-author outbox relays discovered from wire subscriptions, not just bootstrap relays. The diagnostics view shows as much detail as possible about relays, subscriptions, connectivity, and events. The diagnostics relay list deduplicates across both relayStatuses and wireSubscriptions using a seen Set. On Android, the Diagnostics LazyColumn keys relay entries on "${role}:${relayUrl}" instead of relayUrl alone, preventing duplicate-key crashes when the same relay URL serves multiple roles.

relay_statuses() includes entries for all unique outbox relay URLs from wire_subs that are not covered by bootstrap relay roles, and includes the Wallet relay lane if it is dialled and not offline. Outbox relay URLs not present in model.relayStatuses are represented with a synthetic RelayStatus constructed from wire subscription data. Bytes Rx/Tx are shown in the relay detail UI only when the value is greater than zero, avoiding misleading zero displays for outbox relays.

The relay panel shows full relay URLs with ●/○/◌ status indicators and a live event counter per relay, right-aligned in dim text.

Tapping a relay row in the Diagnostics view navigates to a RelayDetailView showing full relay status (connection, auth, bytes RX/TX, timestamps, last notice, last error) plus filtered wire subscriptions and logical interests for that relay. The wire subscriptions section is removed from the top-level DiagnosticsView and instead shown per-relay within RelayDetailView.

Tapping a wire subscription row in the diagnostics list navigates to a detail view showing the subscription's contents. Tapping a subscription shows the event count received on that subscription and the filters used.

DiagnosticsView must use native List with Section headers instead of custom ScrollView cards, and DiagChip capsule badges must be removed.

Chirp must provide the ability to view which relays an event was received from (relay provenance UI).

The chirp-tui OutboxLine snapshot type must include a nested relays: Vec<OutboxRelayLine> field, where OutboxRelayLine contains relay_url, status_label, reason, and message. The chirp-tui Settings outbox pane must support selecting an outbox item (via Enter toggle, j/k navigation, Esc to clear) and displaying a per-relay breakdown showing URL, status dot, reason, and message in a detail sub-pane.

Phase 2 (FFI/wasm snapshot surface for routing trace) is complete. Phase 3 (Chirp iOS/web inspector UI) has not started.

<!-- citations: [^582fc-5] [^582fc-6] [^982ef-1] [^5d893-1] [^86221-3] [^fbebb-1] [^594b7-2] [^42252-1] [^19e07-4] [^93c59-2] [^855be-3] -->
## Wire Subscription Status

WireSubscriptionStatus includes wireId, relayUrl, filterSummary, state, logicalConsumerCount, openedAtMs, lastEventAtMs, eoseAtMs, closeReason, and eventsRx fields. WireSubscriptionStatus.eventsRx is declared as UInt64? (optional) in Swift for backwards compatibility with older kernels that do not emit the field. Each wire subscription tracks an events_rx counter that increments by one for every EVENT frame received on that subscription ID. [^582fc-7]

wire_subscriptions() projects sub.relay_url (the actual resolved outbox URL) instead of sub.role.url() (the bootstrap URL). [^42252-3]

<!-- citations: [^582fc-7] [^42252-2] -->
## Logical Interest Status

LogicalInterestStatus includes key, state, refcount, relayUrls, cacheCoverage, and warmingUntilMs fields. [^582fc-8]

## Wire Subscription Detail View

The wire subscription detail view displays the wire ID, state, relay URL, logical consumer count, opened-at time, EOSE time, last event time, full untruncated filter summary, and close reason. The close reason section is only shown when a close reason is present. Timestamps with a value of 0 are treated as not yet recorded and display a placeholder instead of computing a relative date from Unix epoch. [^982ef-2]

## Relay Settings View

RelaySettingsView must use native Form with Section headers, standard Toggle, and standard Button instead of GlassCard, ChirpPrimaryButton, capsule badges, Color.purple, and custom row backgrounds. [^5d893-2]
## See Also

