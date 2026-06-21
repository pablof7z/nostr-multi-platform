---
title: Relay Roles
slug: relay-roles
topic: marmot
summary: The relay role is additive â a relay can have any combination of Read, Write, Indexer, and Wallet capabilities simultaneously
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-06-18
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:87fd49fb-4869-4c40-9a6a-96545bd2313d
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:50510273-d1c9-424a-b877-179d52fba557
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:c4b2e655-ca6b-42d2-9383-89bf52215d0a
  - session:019edbff-8164-7a20-abc2-c977bc495d49
  - session:019edc4d-4175-7441-b5af-cb2012068335
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Relay Roles

## Relay Roles

The relay role is additive — a relay can have any combination of Read, Write, Indexer, and Wallet capabilities simultaneously. Indexer is a configurable relay role. Wallet (app relay) is a configurable relay role. The `has_role` function must not treat "indexer" as semantically including "write"; indexer relays are only matched when explicitly requesting the "indexer" role.

Relays for the NMP kernel must be provided by the app side, not hardcoded in the kernel. The production Rust kernel must contain zero hardcoded relay URLs or pubkeys; all such constants are restricted to #[cfg(test)] blocks only. All production relay management call sites (actor/relay_mgmt.rs, kernel/outbox.rs, kernel/requests/*.rs, kernel/status.rs, subs/lifecycle.rs) must read from the kernel config instead of hardcoded constants.

Default relays are wss://relay.primal.net (both) and wss://purplepag.es (indexer). The content relay (wss://relay.primal.net) role must be "both" (read+write only), not "both,indexer", in both Rust and TypeScript configs. (Previously: Chirp defaults to wss://purplepag.es for the indexer relay and wss://relay.primal.net for the app/content relay.)

In the Rust backend, relay roles are stored as a space-separated capability list (e.g. "indexer read write"). normalize_roles() parses, deduplicates, and sorts role tokens. has_role() checks role containment with backward compatibility for legacy "both" entries.

The kernel's bootstrap_urls_for_role(role) function reads from the app-provided relay_edit_rows config and returns URLs matching the requested role, returning an empty set if nothing is configured. The kernel's bootstrap_discovery_relays() function returns the union of indexer and content URLs from relay_edit_rows. When no cached kind:10002 relay list exists for an author, the kernel falls back to the app-provided indexer discovery relay rather than a hardcoded constant.

All publishable events must be published to write relays, regardless of kind. Discovery-kind events (kind 0, 3, and kinds 10000–19999) must additionally be published to the user's configured indexer relays. An `is_discovery_kind(u32)` function must return true for kind 0, kind 3, and kinds 10000–19999.

`OutboxResolver::resolve` must accept a `kind: u32` parameter to differentiate event kinds for indexer relay fan-out. `Nip65OutboxResolver` must hold an `Arc<Mutex<Vec<String>>>` for indexer relays that the kernel keeps current via `set_relay_edit_rows`, preventing staleness on relay config changes.

NIP-17 DM-inbox relays must not be pruned by the NIP-65 optimizer; `RoutingSource::Nip17DmRelay` is included in `relay_bypasses_selection` in nmp-planner/src/selection.rs. The P7 finding that NIP-17 should use `relay_pin` is not a defect; `relay_pin` is a static single-URL hard pin, while `PTagRouting::Nip17DmRelays` is a dynamic per-#p lookup against the kind:10050 cache that must fail-closed when unknown.

<!-- citations: [^87fd4-2] [^87fd4-3] [^fd809-3] [^50510-1] [^fe79b-10] [^c4b2e-8] [^019ed-3] [^019ed-87] [^11850-122] -->
