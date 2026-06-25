# Post-v1 milestones

> Part of the [Build & Validation Plan](../plan.md).

These milestones are deferred out of the current v1 ladder. They are not dropped — they are sequenced after [M17](m17-release.md). This file is a temporal post-v1 tracker, not durable product documentation; durable subsystem behavior belongs in `docs/product-spec/` and `docs/design/`.

## Post-v1 M9 — NIP-17 DMs + NSE

See [`m9-messaging.md`](m9-messaging.md) for the full milestone spec (scope, subsystem deliverables, exit gate). **Deferred reason:** DMs add NSE, gift-wrap, NIP-44, App Groups, and a whole capability lane that are not load-bearing for v1 doctrine proofs. The outbox planner's structural ban on routing private events to non-inbox relays is implemented in [M2](m2-subscription-compilation.md) regardless — so the routing contract is already enforced at v1; DMs slot in cleanly when this milestone runs post-v1.

## Post-v1 M12 — Wallet (NWC + zaps + Cashu + nutzaps)

See [`m12-wallet.md`](m12-wallet.md) for the full milestone spec.

**What v1 ships (owner decision 2026-06-12):** the current zap capability is sufficient for v1 — send via NWC (`nmp-nip47`), LUD-16 LNURL fetch (`nmp-nip57`), kind:9735 ingest + `ZapsAggregateProjection`, E2E runtime harness verified (PR #1076, F-04 #978 closed). No further zap work is required before v1.

**Explicitly post-v1 (same owner decision):**
- Receipt `nostrPubkey` author verification — [#1043](https://github.com/pablof7z/nostr-multi-platform/issues/1043) (V-113; already labeled `phase:post-v1`)
- `ZapRequestBuilder` sentinel-value API fix — [#610](https://github.com/pablof7z/nostr-multi-platform/issues/610)
- `zap_subscription` typed-sidecar shape decision — [#1022](https://github.com/pablof7z/nostr-multi-platform/issues/1022)
- Any zap UX hardening
- Cashu / nutzaps (NIP-60/61) — [#1001](https://github.com/pablof7z/nostr-multi-platform/issues/1001)

**Deferred reason for remainder:** wallet is large surface area and the unbuilt portions (Cashu, NIP-60, NIP-61, receipt verification design) are not load-bearing for v1 kernel-boundary proofs.

## Post-v1 Web/WASM — Browser host + wasm parity

See [`m15-cross-platform.md`](m15-cross-platform.md) for the post-v1 web follow-on list. **Deferred reason:** v1 proves the Rust-owned kernel across the native platform contract: iOS, Android, and desktop. Browser delivery needs a production `nmp-wasm` host, OPFS-SQLite persistence, NIP-07 signer wiring, browser consistency fixtures, and honest degraded-mode behavior before the framework claims web support. The browser runtime composition separation and architectural cleanups live in epic [#2045](https://github.com/pablof7z/nostr-multi-platform/issues/2045) (ADR-0067); implementation tactical queue continues in [#1007](https://github.com/pablof7z/nostr-multi-platform/issues/1007).

## Post-v1 Marmot — MLS-over-Nostr Encrypted Groups

See [`marmot-mls.md`](marmot-mls.md) for the full milestone spec. **Deferred reason:** M11.5 explicitly excludes encrypted groups; Marmot is the resolution path. Depends on M11.5's `RelayPinned` routing lane (ADR-0012), M6 signers, M5 NIP-42, and M3 persistence — all v1 deliverables — so the crate shape is clear but the implementation slot is post-v1. **Implementation note:** wraps [`marmot-protocol/mdk`](https://github.com/marmot-protocol/mdk) (v0.7.1+) as `nmp-marmot`; MLS ratchet state uses `mdk-sqlite-storage` alongside NMP's LMDB event store. **Relationship to deferred M9:** coexists — different interop requirements, different threat models. Marmot `Welcome` messages share the NIP-59 gift-wrap transport with NIP-17; the Marmot milestone either follows post-v1 M9 or ships a standalone `nmp-nip59` crate as its Step 0.
