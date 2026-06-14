---
title: Mobile Build Config
slug: mobile-build-config
topic: mobile-build-config
summary: iOS device builds require IPHONEOS_DEPLOYMENT_TARGET=17.0 to avoid a ___chkstk_darwin linker error (unavailable at iOS 10.0 baseline) caused by the Xcode 26 SDK
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:10fcbaec-12f8-4c59-9c2d-38d1c1f7a9c2
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# Mobile Build Config

## Build Configuration

iOS device builds require IPHONEOS_DEPLOYMENT_TARGET=17.0 to avoid a ___chkstk_darwin linker error (unavailable at iOS 10.0 baseline) caused by the Xcode 26 SDK's cc-rs C compilation targeting iOS 26.5. The iOS ChirpTest target has pre-existing compile failures from stale ProfileCard.npub/npubOffset references (removed by ADR-0032/V-115) and a missing import SwiftUI in EmbedKindProjectionTests; these do not block the app archive. The justfile only has a SIMULATOR Chirp build (rust-ios-sim); there is no committed iOS device/TestFlight build invocation, so device archives must set --features marmot and IPHONEOS_DEPLOYMENT_TARGET explicitly. The Chirp iOS shared scheme gets clobbered by xcodegen generate if not protected by a schemes: block in project.yml; this happened at commit 617f477 and was the root cause of hl's Xcode Cloud build failure. HL's Xcode Cloud builds require a shared Highlighter.xcscheme committed to the repository; xcodegen must include a schemes: block in project.yml so the scheme is regenerated as shared rather than getting clobbered. After xcodegen runs for iOS device builds, xcodegen's freshly-generated pbxproj must be kept (not reverted), because the committed pbxproj omits the file reference for gitignored BuildInfo.generated.swift, and restoring it causes a 'cannot find BuildInfo in scope' Swift compile error. The iOS archive for Chirp device build succeeded at /tmp/Chirp-device.xcarchive, producing a 12 MB IPA distribution-signed for App Store Connect upload; the provisioning profile is iOS Team Store for the io.f7z.chirp bundle. The Chirp iOS TestFlight upload requires the App Store Connect issuer UUID which is not on disk (only the API key AuthKey_9HUH4HRW25.p8 is available locally). Android builds target arm64-only because x86_64 is broken on the pre-existing OpenSSL/sqlcipher vendored build issue under NDK 26.1 (issue #1218); the APK must contain both libnmp_android_ffi.so and libnmp_marmot.so under lib/arm64-v8a/ with no x86_64 libraries. The built Android APK is placed at ~/Builds/chirp-debug-arm64.apk. Android Gradle builds must use RUSTUP_TOOLCHAIN=stable to work around a broken nightly install that is missing the rustc binary. build.gradle.kts must be reverted to its original state (including abiFilters and -t flags) after Android builds. The Android --features marmot CI gate checks that marmot builds link correctly with vendored OpenSSL and zeroize std feature. iOS relay seeding must delegate to an `nmp_app_seed_default_relays` FFI symbol backed by `nmp_chirp_config::chirp_default_relay_bootstrap()` rather than hardcoding URLs and parsing JSON in Swift. The codegen Swift drift gate must be a pure byte-diff with zero fuzzing; generated .swift files should be excluded from trailing-whitespace trimming via .editorconfig and .gitattributes to prevent re-drift from flatc's trailing-space output. The Kotlin flatc-drift CI gate only covers nmp/transport/*.kt, not the hand-written nmp/kernel/*.kt bindings, so schema changes to those bindings could silently diverge.

<!-- citations: [^10fcb-1] [^10fcb-2] [^02745-36] [^da6b1-46] [^da6b1-70] [^02745-104] [^10fcb-3] [^da6b1-83] [^78c8e-107] -->
