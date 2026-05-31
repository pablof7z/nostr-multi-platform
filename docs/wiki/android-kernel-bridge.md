---
title: Android KernelBridge & Snapshot Envelope
slug: android-kernel-bridge
summary: "The kernel snapshot envelope format is {\\\\\\\\\\\\\\\\\\\\\\\\\"t\\\\\\\\\\\\\\\\\\\\\\\\\":\\\\\\\\\\\\\\\\\\\\\\\\\"snapshot\\\\\\\\\\\\\\\\\\\\\\\\\",\\\\\\\\\\\\\\\\\\\\\\\\\"v\\\\\\\\\\\\\\\\\\\\\\\\\":{...}} JSON, and the Kotlin model must unwrap this envelope before decoding the inner KernelUpdate da"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:e2d58641-a6c3-4f43-94c0-b018c8fbb893
  - session:64c4fde3-6f5e-456a-b4bb-9f17517e301c
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:c8c2902c-43a6-4b1c-8215-1732dc266895
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:cd331450-f93f-48d0-960e-3c73e927775e
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Android KernelBridge & Snapshot Envelope

## Kernel Snapshot Envelope Format

The kernel snapshot envelope format is {"t":"snapshot","v":{...}} JSON, and the Kotlin model must unwrap this envelope before decoding the inner KernelUpdate data. The SharedSnapshot parser must unwrap the {t:snapshot,v:...} wire envelope before reading projections and metrics fields. Projections live at v["v"]["projections"]["key"]. Android `decodeProjections()` must extract `dm_inbox` and `wallet_status` or DM and Wallet screens remain permanently empty.

<!-- citations: [^86221-1] [^e2d58-4] [^64c4f-1] [^86221-2] [^f3d8d-3] -->
## KernelBridge Native Interface

The `nmp-app-gallery` Rust crate must export JNI wrapper symbols, gated behind an `android-ffi` feature, so the Android `KernelBridge.kt` can load the native library. [^c8c29-1]


The Android Kotlin transport validation files must contain the FLATBUFFERS_25_2_10() runtime guard call. [^37e35-1]

The Android NFCT decoder maps the Invoice type to PlaceholderNode to match the existing generic JSON path behavior, avoiding the invention of a new type. [^cd331-1]
## ProfileWire Kotlin Model

The `ProfileWire.kt` `npub` and `npubShort` fields must be optional with default values so the app does not crash when the kernel omits them. [^c8c29-2]
## See Also

