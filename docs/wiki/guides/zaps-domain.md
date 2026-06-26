---
title: Zaps Domain
slug: zaps-domain
topic: marmot
summary: ZapsDomain registration requires net-new multi-PR ZapsView infrastructure because every premise in the original brief was wrong
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-05-27
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:7b06d382-8fc2-4d52-bef5-f4d90e38cb2a
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# Zaps Domain

## ZapsDomain Registration

ZapsDomain registration requires net-new multi-PR ZapsView infrastructure because every premise in the original brief was wrong. <!-- [^95d02-18] -->

The ZapReceiptsRuntimeController host wiring goes in a sibling controller module (zap_receipts_runtime.rs), NOT in register.rs at app-init time, because register.rs fires before sign-in. <!-- [^1670f-19] -->


The NWC URI `nostr+walletconnect://53e246c2...` contains real money and must only be used for 1-sat test zaps. <!-- [^7b06d-5] -->
## Interest Configuration

self_zap_receipts_interest() uses InterestScope::Global + PTagRouting::Nip65ReadRelays (NOT ActiveAccount + Nip17DmRelays) because the planner's cold-start fallback only fires for that combination. self_zap_receipts_interest_id() takes no pubkey argument (returns InterestId, not String), mirroring the single-slot reconciler pattern from NIP-17 so account-switch replaces the standing subscription instead of accumulating one per pubkey. <!-- [^1670f-20] -->

`wait_for_zap_receipt` correlates kind:9735 receipts on the `bolt11` tag, not just on the recipient pubkey (which would produce false positives for popular recipients). <!-- [^7b06d-6] -->

## Visible Zap Counts

Zap totals are relation data for declared visible targets. NMP must not expose
an app-lifetime or process-wide zap aggregate over every accepted kind:9735
receipt. Note cards and detail views acquire zap counts through
`nmp.nip01.visible_note_relations` or an equivalent bounded visible-target
contract keyed by `#e=<event_id>`.

## LNURL Encoding and Injection

The `lnurl` tag in kind:9734 zap requests MUST be the bech32-encoded LNURL (e.g. `lnurl1dp68gurn8...`), NOT the raw lightning address. The LNURL callback URL MUST include `&lnurl=<bech32_lnurl>` as a query parameter per NIP-57 Appendix B. `url_to_bech32_lnurl` encodes an https URL as a bech32 LNURL string and rejects non-https inputs. `inject_lnurl_tag` in `nmp-nip57` computes the bech32 LNURL from the address and injects it into the kind:9734 unsigned event's tags before signing. `fetch_lnurl_invoice_blocking` includes the `&lnurl=` bech32 parameter in the LNURL callback URL. `fetch_bolt11_for_zap` computes the bech32 LNURL from the well-known URL and passes it to the `ZapRequestBuilder.lnurl()` method. <!-- [^7b06d-7] -->

## Zap Receipt and Payment Flow

NIP-57 kind:9735 zap receipts are published by the recipient's LNURL server ONLY AFTER the invoice is paid. The thin-shell rule requires that iOS/TUI shells must not handle bolt11 or LNURL — the backend owns all protocol logic. The user's design intent is that the app command should simply be `zap` and the backend handles the entire flow end-to-end including NWC payment. chirp-tui `:zap` command usage is `:zap <recipient-pubkey-hex> <lnurl-or-address> <sats> [comment...]`. The V-41 design had `FetchLnurlInvoiceCommand` deliberately NOT chain NWC payment — it only emits a `ShowToast` with the bolt11 and the host was supposed to pay separately, but no host code ever did. V-43 wires the full zap-pay chain in the backend: `FetchLnurlInvoiceCommand` worker success leg calls `nmp_nip47::active_wallet_runtime()` and dispatches `WalletPayInvoiceCommand` with the bolt11. `nmp-nip47` is added as a dependency of `nmp-nip57` to enable the V-43 pay chain. The `nmp.nip57.zap` action stage closes only when the kind:23195 NWC confirmation arrives, not when the bolt11 invoice is fetched. The app-level `:zap` command dispatches `nmp.nip57.zap` and observes the action stage terminal — the backend owns the entire LNURL → NWC payment flow. When no wallet is installed, `FetchLnurlInvoiceCommand` emits `RecordActionFailure` with a descriptive reason instead of leaving the action stage hanging. `WalletPayInvoiceCommand.correlation_id: None` is reserved for actor-internal auto-dispatched payments such as the LNURL → pay_invoice chain. <!-- [^7b06d-8] -->


V-78 (MEDIUM) tracks bunker (NIP-46) accounts being unable to zap because kind:9734 requires local keys and ADR-0026 Phase 2 broker signing is not started. <!-- [^cd2b6-14] -->
## Zap Input and Invocation

Selecting "Zap" from the command palette or pressing the `z` key opens an input bar prompting for `sats [comment]` instead of showing a static help message. AppState holds `pending_zap_pubkey` and `pending_zap_event_id` fields to track the zap target between the palette/key action and the input-bar submission. The app layer sends only `{ recipient_pubkey, amount_msats, target_event_id?, comment? }` for zaps — no LNURL, no lightning address, no protocol details. ZapInput.lnurl is `Option<String>`; shells omit it and the kernel resolves it from the profile cache. When no lnurl is provided, FetchLnurlInvoiceCommand resolves it via `ctx.lnurl_for_pubkey()` (backed by Kernel::lnurl_for_pubkey and ProtocolCommandContext::lnurl_for_pubkey, which read from the kernel's profile cache), failing with a clear toast if the profile has none. <!-- [^95156-3] -->
