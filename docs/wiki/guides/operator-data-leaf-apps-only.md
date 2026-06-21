---
title: Operator Data Belongs Only in Leaf Apps
slug: operator-data-leaf-apps-only
topic: crate-architecture
summary: Hardcoded operator dataâincluding DEFAULT_FOLLOWS pubkeys (such as fiatjaf), DEFAULT_APP_RELAYS, nostrconnect bootstrap relay URLs, and sign_event permissions
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Operator Data Belongs Only in Leaf Apps

## Operator Data Placement

Hardcoded operator data—including DEFAULT_FOLLOWS pubkeys (such as fiatjaf), DEFAULT_APP_RELAYS, nostrconnect bootstrap relay URLs, and sign_event permissions—belongs ONLY in leaf app code, not in NMP itself. Default operator relays, seed follows, nostrconnect bootstrap relays, and NIP-46 permission defaults must not live in nmp-core or nmp-defaults; they must be app-supplied via builder type-state or ActorCommand parameters. nmp-defaults is a reusable NMP composition library, NOT a leaf app; it must not own operator policy such as relay URLs, bootstrap relays, seed pubkeys, auto-follow lists, or signer permissions. DEFAULT_FOLLOWS is deleted from nmp-core; ActorCommand::CreateAccount gains initial_follows: Vec<String> (empty → no kind:3 event published). DEFAULT_APP_RELAYS is deleted. The nostrconnect bootstrap relay becomes NostrConnectBootstrap::Relay|Disabled (fail-observable if unset). Nostrconnect sign_event permissions must be app-supplied via a pre-start config slot with no product default in NMP; Chirp supplies them from nmp-chirp-config. P9 was granted full vertical ownership (option A) for its three coupled breaking changes (relays/pubkeys, known-signers, signer-labels), absorbing P4 Finding 3 (signer label) and P4 Finding 6 (nmp-chirp-config drift).

The P9 nostrconnect permissions PR (PR1b) is sequenced AFTER p5's #1547 because both edit broker/nostrconnect.rs. P9's PR1 #1550 did NOT touch nostrconnect.rs (the perms change was the only thing that did, and it was split out).

Web client.ts must move its ProjectionMergeCache into the wasm worker and generate its relay config from the Rust nmp-chirp-config single source (tracked as #1546, post-v1). <!-- [^11850-253] -->

<!-- citations: [^11850-229] [^11850-230] [^11850-212] [^11850-228] [^11850-240] [^11850-252] -->
## Builder Type-State Enforcement

The builder type-state requires calling .with_relays() or .without_initial_relays() before start() compiles; there is no silent fallback to hardcoded defaults. <!-- [^11850-213] -->
