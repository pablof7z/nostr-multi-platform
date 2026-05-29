---
title: Chirp FFI Boot Sequence & Callback Object Lifetimes
slug: chirp-ffi-boot-and-callback-lifetime
summary: All Chirp platform clients share one FFI boot sequence; raw pointers passed to FFI callbacks must outlive the registration. Use chirp-tui as the reference.
tags:
  - chirp
  - ffi
  - lifetime
  - callback
  - tui
  - desktop
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d1ce3b4a-2a79-40f5-ba0e-fe608f5c7884
---

# Chirp FFI Boot Sequence & Callback Object Lifetimes

> All Chirp platform clients (TUI, desktop, iOS, etc.) share the same FFI boot sequence and action/projection model. When wiring up a new client, `chirp-tui`'s `AppRuntime` is the authoritative reference for both the boot sequence and safe callback lifetime management.

## Boot Sequence

- Every client calls the same FFI initialization entry points in the same order; do not invent a new sequence for a new platform target.
- Consult `chirp-tui` for the exact call order: FFI init → register update callback → start event loop.
- The action/projection model (dispatching actions into the core, receiving projections back via callback) is identical across platforms.

## FFI Callback Object Lifetimes

- `nmp_app_set_update_callback` (and similar registration functions) store a **raw pointer** to a Rust object internally.
- The Rust object behind that pointer **must remain alive** for the entire duration of the registration — from the moment the callback is registered until it is explicitly unregistered.
- **Pattern to follow:**
  1. Store the object in the owning struct (e.g., a field on `AppRuntime` or its desktop equivalent).
  2. Unregister the callback (or ensure the FFI layer is torn down) **before** the owning struct is dropped.
  3. Never pass a pointer to a stack-local or temporary object.
- Failing to do this causes use-after-free: the FFI layer will call back into deallocated memory.
- `chirp-tui`'s `AppRuntime` demonstrates the correct pattern — read it before implementing callback registration in any new client.

## See Also
- [[nmp-desktop-deleted-use-chirp-desktop|nmp desktop deleted use chirp desktop]] — related guide
- [[nfct-native-decoder-not-ffi|Typed FlatBuffers Decoders Must Use Native Platform Bindings — Never a Rust→JSON FFI Hop]] — related guide
- [[chirp-desktop-feature-parity|Chirp Desktop Feature Parity — What Landed and Remaining Gaps]] — related guide
- [[account-operations-c-abi-symbols|Account Operations Must Use Bespoke C-ABI Symbols — Not dispatch_action]] — related guide
- [[desktop-session-storage-file-based|Desktop Session Storage Must Be File-Based — Not OS Keychain]] — related guide

- [nmp-desktop-deleted-use-chirp-desktop](#nmp-desktop-deleted-use-chirp-desktop)
