---
title: DM Relay Ingest and Compile Triggers
slug: dm-relay-ingest
topic: dm-relay-ingest
summary: "The kind:10050 DM-relay-list ingest now triggers CompileTrigger::DmRelayListChanged (the production seam was previously unwired), fixing a bug where fresh accou"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
---

# DM Relay Ingest and Compile Triggers

## DM Relay List Ingest

The kind:10050 DM-relay-list arrival previously never triggered a planner recompile; the DM cold-start closure gate caught a genuine production defect: CompileTrigger::DmRelayListChanged had zero production enqueue sites, meaning a fresh account receiving a kind:10050 DM relay list after inbox interest was pushed never triggered planner recompile. (Previously: the planner recompile was described as already triggered via DmInboxRelayLookup snapshot-diff in the wildcard ingest arm.) The fix mirrors the kind:10002 on_mailbox_changed pattern in the wildcard ingest arm: the IngestParser::parse wildcard arm snapshots recipient_dm_relays before and after verify_and_persist, enqueuing CompileTrigger::DmRelayListChanged when the cache transitions. The current snapshot-diff pattern is O(caches) per ingested event, snapshotting mailbox_cache and recipient_dm_relays before/after verify_and_persist for every event to detect mutations the substrate parser already knows it made; the right shape is for EventIngestDispatcher parsers to return a dirty/changed signal. The DM cold-start verification test drives a real NmpApp kernel via register_defaults with a kernel-compiled kind:1059 REQ (not hand-rolled), verified against wss://relay.primal.net, confirming that gift-wrapped DMs published while a recipient is offline are correctly backfilled via EOSE and decrypted by DmInboxProjection on cold start. Note that DM receive on fresh-install cold-start has not been verified end-to-end on live relays; the existing E2E test bypasses nmp_app_start and writes keys directly, and bunker receive is explicitly broken because inbox.rs requires raw Keys.

<!-- citations: [^da6b1-6] [^f1b74-2] [^da6b1-27] [^da6b1-47] [^da6b1-62] [^da6b1-98] -->
