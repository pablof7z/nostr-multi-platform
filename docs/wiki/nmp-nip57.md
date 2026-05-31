---
title: "NMP NIP-57 Crate: Zap Requests & Receipts"
slug: nmp-nip57
summary: "The `nmp-nip57` crate provides `ZapRequest::to_pubkey(...).amount_msats(...).relays(...).build(...)` for kind 9734, a `ZapReceiptRecord` decoder for kind 9735,"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:156aa64b-42e1-4d3b-96ce-25b31fc06fec
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:95156e27-58fe-4e26-9530-1778033c4559
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# NMP NIP-57 Crate: Zap Requests & Receipts

## API

The `nmp-nip57` crate provides `ZapRequest::to_pubkey(...).amount_msats(...).relays(...).build(...)` for kind 9734, a `ZapReceiptRecord` decoder for kind 9735, and a `bolt11::amount_msats` HRP parser using the bech32-forbids-`1` invariant. The LNURL POST stage is missing at `nmp-nip57/src/action.rs:21-32`, and the `lud06` bech32 path needs the `bech32` dep added to the workspace. Zap receipt subscription logic belongs in `nmp-nip57`, not the kernel — `nmp-core` must know nothing about zaps (kind:9735). The app layer (chirp-tui) must not know about LNURL or zap protocol internals; it only specifies recipient_pubkey, amount_msats, optional target_event_id, and optional comment when zapping. `ZapInput.lnurl` is `Option<String>` — shells omit it and the kernel resolves it from the profile cache. `FetchLnurlInvoiceCommand` resolves the lnurl from the kernel's profile cache via `ctx.lnurl_for_pubkey()` when none is provided, failing with 'this user has no lightning address in their profile' if the profile has none. `Kernel::lnurl_for_pubkey` and `ProtocolCommandContext::lnurl_for_pubkey` are the resolution path for lnurl from a pubkey. If the author has no lightning address in their profile, the kernel surfaces a clear toast to the app. NIP-46 bunker accounts can zap because the zap request gets signed via NIP-46 like any other event signature; no special zap path is needed.

<!-- citations: [^590ca-7] [^1c093-19] [^1670f-12] [^95156-3] [^4edd4-25] -->
## Flow & Integration Gaps

LUD-16 profile resolution is the gate to actual sat movement in the zap flow. The `correlation_id` in the zap flow terminates at kind:9734 publish, not invoice receipt; the tracker must be extended. The Zap button's `NoteRowView.swift:218-223` implementation is currently a no-op stub. ZapAction is wired as a real ActionModule dispatching via `nmp.nip57.zap`, and money-verb actions must not be displayed as a toast placeholder. FetchLnurlInvoice is an ActorCommand implementing ADR-0024. ZapAction records the terminal Accepted stage on LNURL invoice success. Chirp iOS send-zap flow needs to call `nmp.nip57.zap` and display the outcome. ZapsDomain needs kind:9735 receipt registration and ZapsAggregateProjection so received zaps appear in ZapsView. Zaps must be wired into apps correctly with end-to-end LNURL to invoice to pay to kind:9734 flow. Selecting 'Zap' in the command palette opens an input bar prompting for 'sats [comment]'. Pressing the 'z' key triggers the same zap flow as the command palette 'Zap' action.

F-04 Zap E2E is verified live: a real 1-sat payment settled through the project's nmp-nwc code, balance dropped from 1000 to 996 sats. [^4edd4-26]

<!-- citations: [^1c093-20] [^156aa-5] [^2c4ad-12] [^95156-4] -->
## PR Status & Deferrals

PR-C zap LNURL (PR #159) was closed-without-merge, so PR-H (zap completion) is deferred until PR-G lands and PR #159 status is resolved. [^1c093-21]
## See Also

