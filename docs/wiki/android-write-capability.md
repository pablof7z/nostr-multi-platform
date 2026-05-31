---
title: Android Write Capability — Dispatch Door and Write Baseline
slug: android-write-capability
summary: Android had zero write capability until the nativeDispatchAction JNI symbol was added; all writes now flow through this dispatch door.
tags:
  - android
  - chirp
  - write
  - dispatch
  - jni
  - parity
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
  - session:ecf13381-c8ef-40bf-9498-04a1d1f2af8f
---

# Android Write Capability — Dispatch Door and Write Baseline

> Android had zero write capability until the nativeDispatchAction JNI symbol was added; all writes now flow through this dispatch door.

## Overview

Android had zero write capability — `crates/nmp-android-ffi` had no `dispatch_action` JNI symbol. This was the single largest platform parity gap. The B2 work item (`nativeDispatchAction` JNI) is the prerequisite for ALL Android write operations. [^f3d8d-26]

## Dispatch Door (B2)

Android's `nativeDispatchAction` JNI symbol is the dispatch door through which all write operations flow. Once this JNI symbol exists, the Kotlin UI layer can wire compose, react, follow, sign-in, and all other write operations through it. The B2+nav task delivered `dispatchAction` + `openThread` + `openAuthor` JNI symbols. [^f3d8d-27]

## Navigation Parity

Android read/navigation parity is distinct from write parity. `openThread` and `openAuthor` are read operations that should be tracked separately from write operations. Before the cross-platform push, Android only exposed `openTimeline` (`KernelBridge.kt:29`). [^f3d8d-28]

## Write Baseline (C1)

The Android write baseline includes: compose button + send, `openThread`/`openAuthor` wired in Kotlin UI. The `android-write-baseline` task delivered the Kotlin UI layer wiring for compose button, `openThread` on tap, and `openAuthor` on tap. [^f3d8d-29]

## Account Creation via Dispatch

Android's nativeCreateLocalAccount calls nmp_app_create_new_account directly — not through dispatch_action. Account lifecycle operations (create, sign-in, switch, remove) must use the bespoke C-ABI symbols because no ActionModule is registered for those namespaces. The relay list passed to nativeCreateLocalAccount must use the [[url,role],…] array format, not [{url:…,role:…},…].

<!-- citations: [^f3d8d-30] [^f3d8d-38] [^f3d8d-39] [^ecf13-9] -->
## See Also
- [[chirp-cross-platform-parity-plan|Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work]] — related guide
- [[chirp-desktop-feature-parity|Chirp Desktop Feature Parity — What Landed and Remaining Gaps]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[cross-platform-qa-code-review-workflow|Cross-Platform QA and Code-Review Fan-Out — Build, Run, Review, Synthesize]] — related guide
- [[account-operations-c-abi-symbols|Account Operations Must Use Bespoke C-ABI Symbols — Not dispatch_action]] — related guide
- [[chirp-ios-repost-nip18|Chirp iOS Repost (NIP-18) — Implementation and Wiring]] — related guide

