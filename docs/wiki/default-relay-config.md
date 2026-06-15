---
title: Default Relay Configuration
slug: default-relay-config
topic: default-relay-config
summary: "Chirp ships exactly two default relays from a shared Rust config (nmp-chirp-config): wss://relay.primal.net (role 'both' / app relay) and wss://purplepag.es (ro"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-15
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
---

# Default Relay Configuration

## Default Relay Configuration

Chirp ships exactly two default relays from a shared Rust config (nmp-chirp-config): wss://relay.primal.net (role 'both' / app relay) and wss://purplepag.es (role 'indexer'); iOS and Android share this exact config with full parity, with no Swift-side relay URL hardcoding. The primal relay role configuration belongs at the app level (nmp-chirp-config), not in nmp-core itself. (Previously: relay.primal.net had role 'both,indexer'; it was changed to 'both' (app relay only), leaving purplepag.es as the sole dedicated indexer.) The kind:0 'must not leak to app relay at cold-start' contract (IndexerOnly BootstrapSeed) is obsolete; kind:0 routing to app relays is intended, and the IndexerOnly fallback is dropped. No compiler extension for per-interest indexer-only routing is needed. relay.nostr.band has been entirely removed from the codebase (13 files purged, zero remaining references) because it AUTH-walls anonymous REQs and returns zero data. purplepag.es is AUTH-walled for anonymous bulk queries, returning 0 kind:0 events when queried without NIP-42 authentication; it contributed 0 to the indexer baseline in one measurement run and 294 in another when it happened to answer. When it rejects or CLOSEs a REQ and primal.net doesn't hold a given author's kind:0, the profile silently never resolves — amplifying the single-tier indexer dependency. Observed baseline: 10.2% of follows resolve kind:0 via indexer-only path (primal.net alone, since purplepag.es is AUTH-walled to anonymous REQs), rising to 50.0% with the outbox model (indexers ∪ each follow's own write relays), then 60.3% when purplepag.es was responding, and 88.8% when a broad app relay (nos.lol) was added to the outbox model. Of the 300 follows resolved by adding an app relay on top of the outbox model, 204 publish no kind:10002 at all — they are structurally unreachable by the outbox/NIP-65 path and can only be reached via an app/content relay. purplepag.es is hardcoded as CHIRP_INDEXER_RELAY_URL in production, while nmp-core's FALLBACK_INDEXER_RELAY is gated behind test/test-support only, not a production default. Chirp's relay default role is 'Both', not 'Index'; a relay added without the indexer role does not participate in kind:0/kind:100002 discovery. Flipping primal to app-only would have caused a regression for kind:10002 discovery (the D3 probe was gated on indexer_relays being non-empty and emitted only to indexers), so the kernel was fixed to target the probe to indexer_relays ∪ app_relays (deduplicated). This ensures that making primal.net an app relay does not kneecap NIP-65 discovery when purplepag.es is the lone indexer and AUTH-walls anonymous queries — general app relays like primal serve kind:10002 fine. NMP's production code hardcodes no relay URLs; the app supplies relays via nmp_app_add_relay, and the FALLBACK_INDEXER_RELAY = "wss://purplepag.es" is gated behind #[cfg(test)] as test-only. NMP's transport pool dials arbitrary relay URLs on demand via Temporary connections with idle teardown, so connecting to a third-party author's relays requires zero new transport capability. Chirp queries kind:0 on both primal.net and purplepag.es (plus the author's NIP-65 write relays once cached), not purplepag.es alone. The NIP-60 wallet crate (nmp-nip60/relay.rs:104-141) hardcodes wss://purplepag.es as an indexer relay for NIP-65 discovery, bypassing the kernel's D3 outbox router for kind:10002 discovery; this is a minor low-priority follow-up item, not part of the profile migration. A greedy weighted max-coverage set-cover relay selection exists in crates/nmp-planner/src/selection.rs and runs on every recompile, bounded by select_max_connections/select_max_per_user; no new set-cover work is needed.

<!-- citations: [^ab806-1] [^ab806-2] [^ab806-10] [^ab806-27] [^ab806-57] [^ab806-96] [^ab806-127] [^ab806-132] [^ab806-176] [^ab806-186] [^ab806-202] [^ab806-225] [^ab806-236] [^ab806-241] [^ab806-252] [^ab806-263] -->
