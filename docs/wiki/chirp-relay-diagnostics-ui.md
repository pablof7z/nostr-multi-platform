---
title: Chirp Per-Relay Diagnostics UI
slug: chirp-relay-diagnostics-ui
summary: Per-relay diagnostics must allow tapping a relay row to see wire subscriptions sent, events received, notices, errors, bytes RX/TX, and logical interests for th
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-26
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:bbafe8a2-8814-4625-83b6-6af3d4ec0412
  - session:982efe9c-8edb-4ac6-b82a-aa5fef120c9a
  - session:42252c03-76ca-449c-9cfd-ed5949b2bb9d
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
---

# Chirp Per-Relay Diagnostics UI

## Per-Relay Diagnostics

Chirp defaults to using wss://purplepag.es for the indexer and wss://r.f7z.io as the app relay. Users can configure relays in settings within the Chirp app.

The diagnostics view lists ALL connected relays including per-author outbox relays, not just the two bootstrap roles. The relay list deduplicates using a Set across both relayStatuses and wireSubscriptions to prevent duplicate entries. Each relay row is a NavigationLink to a RelayDetailView; each subscription row in RelayDetailView is a NavigationLink to a WireSubscriptionDetailView. The diagnostics navigation flow is: Diagnostics → Relays → [select one relay] → subscriptions, connectivity stats, notices.

The top-level wireSubscriptionsSection is removed from DiagnosticsView; subscriptions are only visible per-relay in RelayDetailView.

WireSub and WireSubscriptionStatus structs carry an events_rx field (u64) counting per-subscription events received, initialized to 0 when a WireSub is created in req_for_relay(), and incremented via saturating_add on every EVENT frame ingested for that subscription. WireSubscriptionStatus in Swift declares eventsRx as UInt64? (optional) for backwards compatibility with older kernels. Tapping a subscription shows events_rx (event count received), filters used, and other details.

RelayDetailView shows metric tiles for Total Subs, Active, Events Rx, EOSE'd, and Bytes Rx/Tx. relay_statuses() includes entries for unique outbox relay URLs from wire_subs that are not covered by bootstrap roles. Outbox relay URLs without a RelayStatus entry get a synthetic RelayStatus with role='outbox', connection='connected' when active subs exist, and bytes only shown when > 0. wire_subscriptions() projects sub.relay_url (the real resolved outbox URL) instead of sub.role.url() (the bootstrap URL). RelayStatus.bytes_rx and bytes_tx remain u64 (not Option) in Rust; the Swift UI hides them when value is 0.

The wire subscription list rows must display wire ID, state chip, truncated filter summary, and relay URL. Tapping a wire subscription row navigates to a detail view showing all subscription fields.

The wire subscription detail view must display:
- An identity section with the full wire ID (selectable), state chip with color, relay URL (selectable), and logical consumer count.
- A timing section with opened-at relative time, EOSE relative time (or 'pending'), and last event relative time (or 'none').
- A filter section with the full untruncated filter summary text (selectable).
- A close reason section only when present, shown in red.

Timestamp fields with a value of 0 must display as '—' rather than computing a date from Unix epoch.

The diagnostic REPL must show every relay connection attempt, every REQ (including implicit discovery REQs), and every terminal state (EOSE, CLOSED with reason, AUTH challenge, NOTICE) — no relay interaction may be silent. Every implicit REQ (such as mailbox-probe kind:10002) is shown individually, not collapsed into a summary.

The Settings > Relays panel must show real connection state per relay (green dot when connected) rather than always showing a connecting indicator. [^fbebb-1]

<!-- citations: [^582fc-7] [^bbafe-1] [^982ef-1] [^42252-1] [^fd809-1] [^19e07-9] -->

## TUI Outbox Diagnostics

OutboxLine in chirp-tui's feature_snapshot must include a relays: Vec<OutboxRelayLine> field parsed from the kernel JSON. OutboxRelayLine must carry relay_url, status_label, reason, and message fields. Every published event regardless of status (pending, queued, Ok, failed, etc.) must be selectable in the outbox. Selecting any published event in the outbox opens a per-relay detail view showing the status and reason for each relay. The per-relay detail view must display why each relay was selected (e.g., Inbox relay for npub1…, NIP-65 write relay, App relay, Explicit relay). [^fbebb-2]
## See Also

