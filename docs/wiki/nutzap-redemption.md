---
title: Nutzap Redemption and Idempotency
slug: nutzap-redemption
topic: zap-flow
summary: Redemption idempotency is a NIP-61 protocol-level concern and must be handled in the nmp-nip60 crate, not patched per-app
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-07
updated: 2026-06-07
verified: 2026-06-07
compiled-from: conversation
sources:
  - session:b4497e9a-60f0-4e9b-b8bc-6d706ed1426c
---

# Nutzap Redemption and Idempotency

## Nutzap Redemption

Redemption idempotency is a NIP-61 protocol-level concern and must be handled in the nmp-nip60 crate, not patched per-app. The current non-idempotent `redeem_nutzap` behavior is a bug in the nmp-nip60 crate (not just the wallet-poc) and has been filed as GitHub issue #952. <!-- [^b4497-1] -->

The `Nip60WalletHandle` struct must include a `redeemed: Arc<Mutex<HashSet<EventId>>>` field tracking already-redeemed nutzap event IDs. `load_from_relays` must fetch kind:7376 history events and seed the redeemed set from their `redeemed` tags. <!-- [^b4497-2] -->

`redeem_nutzap` must be idempotent — it must short-circuit before contacting the mint when the nutzap event_id is already in the redeemed set, returning a distinct `Nip60Error::AlreadyRedeemed` (or `Ok(0)`) when the nutzap event_id is already redeemed. <!-- [^b4497-3] -->
