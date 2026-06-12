---
title: Zap Send Flow and Feedback
slug: zap-flow
topic: zap-flow
summary: "The zap send flow (Rust side) is complete: ZapAction → FetchLnurlInvoiceCommand → signs kind:9734 → LNURL HTTP → WalletPayInvoiceCommand → NWC kind:23195 → kern"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-01
updated: 2026-06-12
verified: 2026-06-01
compiled-from: conversation
sources:
  - session:89070aba-0e77-4da3-99e1-322addb1c747
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
  - session:63af4b96-d3d3-45c3-ab96-9f899beafa1b
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Zap Send Flow and Feedback

## Zap Send Flow

The zap send flow (Rust side) is complete: ZapAction → FetchLnurlInvoiceCommand → signs kind:9734 → LNURL HTTP → WalletPayInvoiceCommand → NWC kind:23195 → kernel.record_action_success.

Bunker zap signing routes through sign_active_nonblocking on ProtocolCommandContext, resolving SignerOp on-actor and calling op.wait(10s) on the existing HTTP worker thread, matching the DM-send pattern from ADR-0040. ProtocolCommand::run has no access to pending_signs, so a PendingZapSign park is impossible; the correct idiom is resolve SignerOp on-actor then op.wait() off-actor on the HTTP worker.

The V-78 fix (PR #938) merged to master: bunker accounts can now zap via the nonblocking sign path.

The zap command rebuilds the SignedEvent from nested internal shape into flat standard NIP-01 wire format, proven byte-identical to the old local-key path via test. SignedEvent serializes as a nested struct, not flat NIP-01 JSON; the V-78 fix added signed_event_to_nostr_json to produce the flat wire format the LNURL provider expects.

The ZapsAggregateProjection is registered and decoded in Swift.

Timeline zap counts update live via NoteRelationIndex.

The KernelModel.zap() function returns DispatchResult (not discardable) so callers can capture the correlation ID.

HomeFeedView stores the zap correlation ID in a pendingZapCid state variable and observes model.actionLifecycle for terminal stages.

After a zap payment succeeds, the app shows a success toast with haptic feedback.

After a zap payment fails, the app shows the error reason.

The zap success feedback follows the same pattern as DM-inbox publish in RelaySettingsView.

<!-- citations: [^89070-2] [^89070-3] [^89070-4] [^89070-5] [^89070-6] [^89070-7] [^89070-8] [^89070-9] [^f1b74-11] -->
## Toast System

KernelModel exposes showSuccessToast() and clearSuccessToast() to manage a lastSuccessToast published property. <!-- [^89070-10] -->

Success toasts display in green for 3 seconds; error toasts display in gray for 4 seconds. <!-- [^89070-11] -->

## Deferred Work

Zap receipt handling (kind:9735) remains deferred until a zap ADR lands; no action is taken to register ZapsDomain in Chirp for zap receipt processing.
Zap verification/hardening work (receipt nostrPubkey extraction, NWC sentinel API, zap_subscription typed sidecar shape) is declared post-v1 by owner decision, documented across plan.md, post-v1.md, and m12-wallet.md with the four contradictions in those docs resolved. (Previously: Zap receipt verification/hardening is deferred to post-v1 per owner decision.)
NIP-57 zap receipts are author-unverifiable in production because nmp-nip57 reads allowsNostr but never extracts nostrPubkey, making receipts forgeable — this is tracked as V-113 (#1043) and explicitly deferred to post-v1 by owner decision.
The F-04 zap E2E harness verifies the full kernel zap round-trip over a live relay using a scripted fake NWC wallet: production Chirp opens a RelayRole::Wallet socket, wallet pays a kind:23194 invoice, fake wallet decrypts the bolt11 and replies kind:23195, kernel settles, and a Schnorr-verified kind:9735 updates the nmp.nip57.zaps aggregate projection.
A raw-event observer for kind:9735 with no UI consumer (option C) is an anti-pattern to avoid.
zap_subscription returns serde_json::Value::Null on every code path (it is a side-effecting interest reconciler with no read-noun), so it cannot be typed as Wave A producer-typing without a projection redesign.
zap_subscription must be re-homed from the projection registry onto a proper AppHost tick-hook seam (register_snapshot_tick_observer) before the dynamic registry can be deleted.
wallet has no typed producer call site because WalletStatusData.walletPubkeyHex (non-optional, read by WalletView) is emitted by neither the JSON projection nor wallet_status.fbs; a producer field-add is required before the consumer flip.

<!-- citations: [^63af4-7] [^63af4-8] [^63af4-9] [^89070-12] [^89070-13] [^f1b74-12] [^da6b1-39] [^da6b1-85] [^da6b1-94] [^da6b1-113] -->
