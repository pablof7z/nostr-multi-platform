---
title: NMP Relay Selector & Max-Coverage Algorithm
slug: nmp-relay-selector
summary: The relay selector uses a greedy max-coverage algorithm (applesauce-style) with a per-user cap and a global connection budget, not naive fan-out to every declar
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:bbafe8a2-8814-4625-83b6-6af3d4ec0412
  - session:d4b109a1-b655-4952-9e89-9a8a1438d6a2
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# NMP Relay Selector & Max-Coverage Algorithm

## Selection Algorithm

The relay selector uses a greedy max-coverage algorithm (applesauce-style) with a per-user cap and a global connection budget, not naive fan-out to every declared write relay. Personal relay URLs (e.g. per-npub paths like filter.nostr.wine/npub1...) are not explicitly filtered; the selector naturally excludes them because they have coverage=1. Relays known to be dead are excluded from the selection candidate set before apply_selection runs. The outbox planner exposes a RelayScore trait seam with a single method returning Option<f32> from a &ResolvedRelay, with a default implementation returning None. Routing decisions are fed from local RTT and NIP-65 first, rather than untrusted NIP-66 data, to prevent degraded relay selection from stale or biased monitor events. The V-52 single-relay browsing implementation uses the existing InterestShape::relay_pin and planner case_e_relay_pinned rather than adding a redundant scope_relays field, avoiding fragmentation per the project's anti-fragmentation rule.

<!-- citations: [^bbafe-4] [^d4b10-5] [^4edd4-28] -->
## Compilation and Application

The SubscriptionCompiler accepts app_relays, indexer, account_read, and cache via a with_relays constructor. The apply_selection function runs greedy max-coverage selection on the compiled plan, takes max_connections and max_per_user parameters, and calls recompute_hash on mutated shapes without touching plan_id. [^bbafe-5]

## Production Outbox Flow

The production outbox flow is: compile naive plan, strip dead relays, apply_selection (greedy max-coverage), coverage hook, watermark rewrite, wire-emitter diff. [^bbafe-6]

## Request Dispatch

REQs are sent to each relay the moment it connects, independently of whether other relays have finished handshaking. [^bbafe-7]

## Dead Relay Tracking

The SubscriptionLifecycle tracks dead relays via a RelayHealthChanged trigger and filters them from the candidate set on recompile. [^bbafe-8]

## Diagnostic Tool (nmp-repl)

The nmp-repl diagnostic tool drives the real SubscriptionLifecycle (not a manual reimplementation), so it exercises implicit discovery, Nip65Arrived round-trip, apply_selection, and dead-relay filtering for real. It shows every relay connection with its status (connecting, connected, failed), every REQ (including implicit ones like mailbox-probe), and terminal states (EOSE, CLOSED with reason, AUTH challenge, NOTICE) as first-class visible events. No hardcoded relay URLs (other than the single default indexer purplepag.es) exist in nmp-repl; dead relays like relay.nostr.band are not hardcoded. [^bbafe-9]

## Mailbox Probe Batch Size

MAILBOX_PROBE_BATCH is 500 authors per discovery REQ, not 50. [^bbafe-10]
## See Also

