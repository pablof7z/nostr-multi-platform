---
title: Mobile CI and Testing
slug: mobile-ci
topic: mobile-ci
summary: Android JUnit tests run on every PR on ubuntu via native-tests.yml, path-filtered to android/**, nmp-android-ffi, nmp-codegen, nmp-core
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:bbd5fe79-cd71-4de0-ba9f-f3684520a03f
  - session:65edf39e-4cfd-4bfc-9b65-ec4dc1944b1e
  - session:63af4b96-d3d3-45c3-ab96-9f899beafa1b
---

# Mobile CI and Testing

## Mobile CI

Android JUnit tests run on every PR on ubuntu via `native-tests.yml`, running 22 @Test methods across 5 suites (pure JVM, no Rust toolchain needed), path-filtered to `android/`, `nmp-android-ffi`, `nmp-codegen`, and `nmp-core`. iOS XCTests and XCUITests are staged for nightly CI, blocked on a macOS runner prerequisite (keyring entitlement + `just rust-ios-sim`). There is no iOS-build CI gate in the NMP repository; the local iOS simulator build (xcodebuild test) is the authoritative validation for Swift changes. iOS project moves must be done by hand-editing SRCROOT-relative path depths in `project.yml`/`project.pbxproj`, deliberately avoiding `xcodegen generate` to prevent UUID churn. Merging PR #903 requires the human to locally run `xcodegen generate --spec apps/chirp/ios/project.yml` and a one-time Android Gradle/NDK build to confirm. In-device Apple transcript tests are executed via XCTest on a physical iPhone. The `nmp_app_chirp` native library requires building for `aarch64-apple-ios` before running XCTests on a physical iPhone. The Rust build for iOS requires the deployment target to match Xcode's (iOS 17.0) to avoid the `___chkstk_darwin` undefined symbol error from defaulting to iOS 10.0. The real-relay nightly workflow had a pre-existing bug where --features test-support was invalid on nmp-testing, preventing the --ignored suite from running; this was fixed in the F-02 cold-start PR.

<!-- citations: [^da6b1-11] [^bbd5f-4] [^bbd5f-5] [^65edf-2] [^63af4-3] [^da6b1-32] [^da6b1-54] [^da6b1-67] [^da6b1-106] -->
## Android Dark Status at v0.3.0

Android was completely dark at v0.3.0 because KernelUpdateFrameDecoder.kt gated the entire decode on snapshot.payload (now deleted); the #1074 review's 'Android ready for PR-B: YES' ruling was incorrect. <!-- [^da6b1-12] -->

## Android Fix (PR #1092)

PR #1092 fixes Android by rebuilding KernelUpdate entirely from Tier-3 SnapshotEnvelope fields and typed sidecars, with no payload fallback, and includes a real-frame golden test. <!-- [^da6b1-13] -->

## Cross-Platform Frame-Contract CI

Cross-platform frame-contract CI gates exist for Rust and Swift but not for Kotlin or TypeScript bindings; the systemic fix for the Android-dark class is golden-frame fixture tests decoding real kernel-emitted bytes on each platform. <!-- [^da6b1-14] -->
