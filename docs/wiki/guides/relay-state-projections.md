---
title: Relay State Projections
slug: relay-state-projections
topic: marmot
summary: "Relay state fields (indexer_relays, local_write_relays, relay_edit_rows) are actor-owned: the sole write path is via IdentityState::set_relay_edit_rows from act"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-19
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
  - session:e3b42d41-ffd2-44b3-9e5a-93832feb46e0
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
---

# Relay State Projections

## Relay State Projections

Relay state fields (indexer_relays, local_write_relays, relay_edit_rows) are actor-owned: the sole write path is via IdentityState::set_relay_edit_rows from actor commands, with no FFI writer. PR-I (#234) promoted these from raw shared primitives (Arc<Mutex<Vec<String>>> and Arc<Mutex<Vec<RelayEditRow>>>) to typed projection slots (RelayUrls, RelayEditRowList) under projections['relays.edit_rows'], ['relays.indexer_set'], and ['relays.write_set'], and added the D14 lint. The `role_tint` field on `RelayEditRow` is a semantic token (`accent`, `info`, `success`, `neutral`) rather than a hex color string. The four Arc<Mutex<…>> handles for identity-relay state (relay_edit_rows_handle, indexer_relays_handle, local_write_relays_handle, active_account_handle) should be consolidated into a single Arc<Mutex<IdentityRelayState>> to prevent stale cross-field reads and lock-order deadlocks. The diagnostics relay list deduplicates across both relayStatuses and wireSubscriptions using a Set. When an Indexer relay closes, profile pubkeys that were requested but not yet fulfilled are moved back to pending so they are re-batched on reconnect.

The p9 core-config lane has full vertical ownership (option A) for relays/pubkeys, known-signers source-of-truth, and signer-labels — including actor/mod.rs, dispatch.rs, apps, nmp-ffi, schema, and shell files. chirpConfig.ts relay role drift (Rust "both" vs TS "both,indexer") must be unified from a single Rust source (nmp-chirp-config), assigned to the core-config lane.

The Home > Relays pane renders `state.relays` (Vec<RelayRow>), populated from the kernel's live-computed `relay_diagnostics` projection parsed on each tick via `snapshot.rs:96–111` (`relays_from`). The `relay_diagnostics` `*_tone` semantic-token selectors are kept — they emit raw tokens (not colors or prose). The Settings > Relays pane uses a three-tier fallback: primary `state.features.relay_edit_rows` (user-editable list from kernel), fallback to `state.relays` (live diagnostics), final fallback to `relay_lines(state)` (minimal text). Relay connection status labels in the settings pane use live connection state via settings.rs. Bootstrap relay URLs come from `relay_edit_rows` if non-empty, otherwise from hardcoded defaults: content relays → `wss://relay.primal.net`, indexer relays → `wss://purplepag.es`.

Per-relay aggregation (filtering/sorting/summing wire state) in RelayDetailView.swift and DiagnosticsView.swift violates aim.md §4.5 'no derived state' and §6 anti-pattern #1, and should be a RelayDetailSnapshot projection in Rust.

Outbox status display strings (NotificationsView.swift:82-97) that count entries via .filter and assemble English via ternaries violate aim.md §2 anti-pattern #1 (no presentation formatting in Rust) and §6 doctrine rule #9 (no domain logic in native). Correct rule (aim.md §2 #4 discriminating test): Rust ships raw counts `{ total, sending, retrying, queued, failed }` in an OutboxSummarySnapshot; the shell formats the title/subtitle prose — assembling the English sentence is a presentation-only concern that does not need to be reimplemented to stay protocol-correct on a second platform.

When a replaceable event (kind 0, 3, or 1xxxx) is received from a non-indexer relay, the system republishes it to connected indexer relays if store.provenance_for(event_id) shows no indexer relay has already delivered that event. RawEvent::is_replaceable() returns true for kind==0, kind==3, or kind in 10000..20000. The indexer republish feature is optional and defaults to enabled. Republished events are re-serialized from the verified RawEvent (signature validity preserved but not byte-identical to the original). The republish pipeline uses a bounded LRU dedup cache of 4096 entries. The raw_event_observer hook at raw_event_observer.rs:60 fires after Schnorr+id-hash verification and store insert, carrying (raw: &RawEvent, relay_url: &str), and supports KindFilter — it is the D0-clean substrate seam for the republish pipeline.

EventStore.provenance_for(event_id) returns Vec<ProvenanceEntry> with relay_url, first_seen_ms, last_seen_ms, and a primary flag, persisted in LMDB with max 32 relays per event. The relay-kind index is a derived projection written only inside the canonical provenance update (provenance::upsert/delete), never mirrored or mutated independently, satisfying D4 single-writer-per-fact. The backfill for the relay-kind index is gated by a domain-versions key; it scans every stored event to build id→kind, then joins with provenance to write relay_kind entries, skipping private kinds. F-09 backlog entry: all Chirp platforms should display which relays an event was sourced from, using store.provenance_for(event_id) which already exists persistently in LMDB.

<!-- citations: [^1c093-29] [^19e07-9] [^45fcf-13] [^47203-13] [^45258-26] [^86221-8] [^6e4c3-1] [^e3b42-4] [^11850-58] [^11850-167] [^129d2-135] -->
