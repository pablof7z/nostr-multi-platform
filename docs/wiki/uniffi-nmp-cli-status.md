---
title: UniFFI & nmp CLI Status
slug: uniffi-nmp-cli-status
summary: "UniFFI and the `nmp` binary have not shipped: UniFFI has only plain derives, nmp init/gen are lib APIs, and the generated FfiApp.dispatch() is a stub versus the"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-26
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:7f0f0c78-d1aa-49db-b659-c9cf49827117
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:ad1d532e-a335-44fb-827e-a3f0318a3aae
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:53ffd7cf-32db-4347-b6ec-e2d6244f8e58
  - session:8a8aefe0-93b0-405d-99da-0c8ac39114c8
---

# UniFFI & nmp CLI Status

## Shipping Status

UniFFI is a milestone M14 item and must not be implemented now. UniFFI is used for command/action calls and capability hooks to collapse the handwritten FFI surface, not primarily for snapshot transport. The transport strategy must evaluate and decide between JSON, UniFFI, and FlatBuffers formats, and determine whether FlatBuffers provides sufficient benefit to justify its use for wasm targets as well. (Previously: UniFFI and the `nmp` binary have not shipped: UniFFI has only plain derives, nmp init/gen are lib APIs, and the generated FfiApp.dispatch() is a stub versus the live raw-C FFI. From Swift/Kotlin FFI, unsigned events are published by calling `nmp_app_publish_unsigned_event(app, json_ptr)` where `json` is `serde_json::to_string(&unsigned)`. Existing documentation targets UniFFI as a future transport for milestone M14.)

nmp_app_free has a double-free undefined behavior risk with no runtime guard.

nmp_app_gallery_register returns void, not void*.

ffi-surface.md is stale with 5+ undocumented production symbols and NmpCore.h is provisional and likely incomplete.

An FFI surface freeze CI workflow (.github/workflows/ffi-surface-freeze.yml) is staged but not yet merged to prevent C-ABI churn.

<!-- citations: [^7f0f0-15] [^57528-19] [^57528-20] [^590ca-12] [^ad1d5-10] [^2c4ad-14] [^53838-16] [^53ffd-2] [^8a8ae-5] -->
## See Also

