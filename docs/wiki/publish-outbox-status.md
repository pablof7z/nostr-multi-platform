---
title: Publish Outbox Status Tracking
slug: publish-outbox-status
summary: The publish_outbox status function returns 'queued' when at least one relay has accepted (Ok) and others are still Pending, rather than falling through to retur
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-27
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:fa300009-e498-4c80-a2d3-64d1531a09d4
  - session:7174d4d4-371b-4b8e-87a6-91024c2b4c2a
  - session:7e56b660-13cc-42c9-915c-f8f97ef826d9
---

# Publish Outbox Status Tracking

## Publish Outbox Status

The publish_outbox status function returns 'queued' when at least one relay has accepted (Ok) and others are still Pending, rather than falling through to return 'pending'. All events published from the TUI are visible, including kind 10002, kind 0, and other events emitted on account creation.

<!-- citations: [^fa300-2] [^7174d-2] -->

## Relay Selection Reasons

The core contract uses a RelaySelectionReason enum throughout the internal pipeline instead of human-readable strings; format_relay_reason() is the sole display site isolated to publish_outbox.rs. The ResolvedRelay struct and OutboxResolver::resolve() return type Vec<ResolvedRelay> carry relay selection reasons through all five Nip65OutboxResolver code paths. Per-relay publish rationale is surfaced so outbox UIs can show per-relay reasons like 'NIP-65 write relay' or 'Inbox relay for npub1abc…' alongside the existing per-relay status. [^7174d-3]

## Persistence and Eviction

relay_reasons survive eviction from publish_outbox by threading them through TerminalOutcome to RelayAckOutcome so that publish_queue history rows also carry the reason string. InFlight.relay_reasons is write-once through the engine and persistence layer. The kernel projection PublishOutboxRelay includes a relay_reason field in its JSON output. [^7174d-4]


Every outbound EVENT publish is written to PublishStore (filesystem/LMDB) in Pending state before the first send attempt, making the publish path crash-safe. [^7e56b-2]
## TUI Outbox Display

The kernel provides pre-formatted PublishQueueEntry.title so the TUI reads it verbatim instead of duplicating kind-label display logic. The TUI outbox pane supports j/k navigation, Enter to expand, and Esc to collapse per-relay detail in a split pane showing status dot, URL, dim reason, and dimmer message. The TUI renders a read-only Published history section in the Settings outbox pane showing all past publishes with per-relay outcomes. The TUI publish history section is always visible, sorted newest first, and capped at 20 entries. [^7174d-5]

## iOS Outbox Display

iOS PublishOutboxRelay has a relayReason String property that Decodable picks up automatically, defaulting to empty string for old kernels. iOS decoding of relayReason uses a custom init(from:) with decodeIfPresent that defaults to an empty string, preventing decode failures when the key is omitted. iOS OutboxRelayRow displays relay.relayReason as a .caption2/.secondary line below the URL row, guarded by if !relay.relayReason.isEmpty. [^7174d-6]

## Ack Classification and Retry Behavior

When a relay rejects an event with a NIP-20 OK prefix matching a permanent code, classify_ack returns AckClass::Permanent, apply_ack transitions the per-relay state to FailedAfterRetries with no retry, and the publish engine evaluates the overall outcome across all relays. PERMANENT_CODES includes "blocked", "pow", "rate-limited", "restricted", "invalid", "duplicate", and "mute". Unknown or unspecified NIP-20 codes (including the catch-all "error" prefix) fall through to AckClass::Transient, causing the publish engine to retry up to 3 times with backoff before giving up. [^7e56b-3]
## See Also

