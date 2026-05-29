---
title: NIP-47 Wallet Payment Correctness — Encoding Errors and TTL Sweep
slug: nip47-wallet-payment-correctness
summary: NIP-47 payment encoding failures are now surfaced as errors (not silently dropped); pending payment TTL sweeps fire via on_idle_tick even when the NWC relay is silent.
tags:
  - nip47
  - nwc
  - payments
  - v63
  - v64
  - relay-interceptor
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# NIP-47 Wallet Payment Correctness — Encoding Errors and TTL Sweep

> NIP-47 payment encoding failures are now surfaced as errors (not silently dropped); pending payment TTL sweeps fire via on_idle_tick even when the NWC relay is silent.

## V-63: Silent Payment-Frame Encoding Failure

Three sites in `crates/nmp-nip47/src/runtime.rs` called `serde_json::to_string(...).unwrap_or_default()` when encoding REQ, CLOSE, and EVENT frames. An encoding failure silently produced an empty string that the relay received as a malformed frame.

Critical ordering bug: `pending_payments` map insertion happened before `encode_frame`, so a failed encoding registered a correlation_id as in-flight when the relay never received the request.

Fix: replaced all three `unwrap_or_default()` calls with a new `encode_frame(value: &serde_json::Value) -> Result<String, serde_json::Error>` helper; `pending_payments` insertion moved to after successful `encode_frame`. [^42908-31]

## V-64: Pending Payment TTL Sweep and Orphan Observability

`pending_payments` previously stored `Option<String>` with a `(_, None) => {}` catch-all that silently discarded orphan responses.

Fix:
- `pending_payments` value type changed to `PendingPayment { correlation_id, inserted_at_secs }`
- The orphan arm replaced with `tracing::warn!` + `orphan_responses` counter
- `WalletRuntime::sweep_expired_payments(now_secs, ttl_secs) -> Vec<(String, String)>` — pure function returning `(cid, reason)` pairs
- `PENDING_PAYMENT_TTL_SECS` exported from `nmp-nip47/src/lib.rs` [^42908-32]

## D8-Compliant Sweep Wiring

The TTL sweep must fire even when the NWC relay is completely silent. Implemented via an `on_idle_tick(&self, kernel: &mut Kernel)` default-noop method added to the `RelayTextInterceptor` trait (`nmp-core/src/substrate/relay_intercept.rs`). The actor's idle section (`actor/mod.rs`) calls `on_idle_tick` on every loop iteration. `nmp-app-chirp::WalletInterceptor` overrides `on_idle_tick` to call `sweep_expired_payments` and record returned failures via `kernel.record_action_failure`. [^42908-33]

## See Also

