---
title: NWC Parser and Signer Cleanups (V-72, V-74, V-77)
slug: nwc-and-signer-cleanup
summary: "NWC URI unknown params are now surfaced as ParseError::UnknownParam; dead MakeInvoice API deleted; LocalKeySigner kind overflow returns a typed error instead of silent u16::MAX."
tags:
  - nwc
  - nip47
  - signer
  - v72
  - v74
  - v77
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# NWC Parser and Signer Cleanups (V-72, V-74, V-77)

> NWC URI unknown params are now surfaced as ParseError::UnknownParam; dead MakeInvoice API deleted; LocalKeySigner kind overflow returns a typed error instead of silent u16::MAX.

## V-74: Unknown NWC URI Params Are Surfaced

`crates/nmp-nwc/src/parse.rs` previously had a `_ => {}` catch-all that silently discarded unrecognised query parameters from NWC URIs.

Fix: `ParseError::UnknownParam { key: String }` added; the catch-all replaced with an error return. Four new tests cover `relays=`, `Relay=`, and Display output. [^42908-50]

## V-77: Dead MakeInvoice API Deleted

The NIP-47 `MakeInvoice` API surface had no real callers anywhere in the workspace (Rust or Swift/Kotlin). Deleted:
- `NwcMethod::MakeInvoice` variant and `as_str` arm
- `MakeInvoiceParams` struct
- `MakeInvoiceResult` struct
- `make_invoice_content()` function
- Related imports and re-exports

Any future `MakeInvoice` support must be implemented from scratch against a current NIP-47 spec. [^42908-51]

## V-72: LocalKeySigner Kind Overflow Is Typed

`crates/nmp-signers` previously coerced overflowing event kind values to `u16::MAX` silently via `unwrap_or(u16::MAX)`. Fix: overflowing kind returns a typed error. The call site must handle the error explicitly rather than receiving a silent `u16::MAX` sentinel. [^42908-52]

## See Also

