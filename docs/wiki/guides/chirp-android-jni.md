---
title: Chirp Android JNI and EFI Bindings
slug: chirp-android-jni
topic: app-platform-bindings
summary: Android's `KernelBridge.kt` JNI extern declarations use plain `external fun` (public), not `internal external fun`, because Kotlin mangles internal JVM method n
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp Android JNI and EFI Bindings

## KernelBridge JNI Declarations

Android's `KernelBridge.kt` JNI extern declarations use plain `external fun` (public), not `internal external fun`, because Kotlin mangles internal JVM method names with a module-name suffix that the Rust cdylib's exported symbols don't carry, causing `UnsatisfiedLinkError`.

The `nativeResolveRef` and `nativeReleaseRef` JNI wrappers are implemented in `nmp-chirp-android-ffi`, mirroring the existing `nativeClaimEvent`/`nativeReleaseEvent` pattern, backing `KernelBridgeRefs.kt`'s extern declarations and `NostrAvatar` profile resolution on Android.

<!-- citations: [^dcc80-acad5] [^dcc80-1a7c3] [^dcc80-70f7a] -->
