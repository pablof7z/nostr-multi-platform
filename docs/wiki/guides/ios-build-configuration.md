---
title: iOS Build Configuration
slug: ios-build-configuration
topic: ios-build
summary: After adding Marmot symbols to nmp-app-chirp, the iOS simulator static libs must be rebuilt for the aarch64-apple-ios-sim target (via `cargo build --target aarc
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-06-19
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:f003440d-ee18-49d2-aa43-f2e806706008
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:30bf8c76-8be2-4e26-b22d-30ca86c37162
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:45c5d788-6be0-4b50-85da-52ee2538a65d
  - session:63dfcbb3-3ae0-48bb-9228-a494f85df203
  - session:0048057e-cb95-4da0-9f74-039a07dfc89f
  - session:e6b44a84-8cfc-48b2-863a-58382398b5df
---

# iOS Build Configuration

## iOS Build Configuration

After adding Marmot symbols to nmp-app-chirp, the iOS simulator static libs must be rebuilt for the aarch64-apple-ios-sim target (via `cargo build --target aarch64-apple-ios-sim` or the justfile `rust-ios-sim` target). The project uses `~/.cargo/target-shared` as the Cargo shared target directory, requiring a symlink from `target/aarch64-apple-ios-sim` for Xcode to find the built Rust library. Building for a physical iOS device requires building Rust libraries for the aarch64-apple-ios target via the `rust-ios-device` justfile recipe, which performs the full release build with the `marmot` feature enabled and `IPHONEOS_DEPLOYMENT_TARGET=17.0`; these device builds must use `--release` because the project links against release builds rather than dev builds, and the deployment target must be set to 17.0 to prevent a `___chkstk_darwin` linker error that occurs when building nmp-marmot's cdylib target against the newer iOS 26.5 SDK with the Rust target's default iOS 10.0 deployment floor. iOS does not support dynamic libraries (cdylib), which causes nmp-marmot's cdylib crate type to fail with an `ld: symbol(s) not found` error. The pbxproj must use platform-conditional `LIBRARY_SEARCH_PATHS` (`[sdk=iphoneos*]` and `[sdk=iphonesimulator*]`) to prevent the linker from picking up the wrong architecture's static libraries; in `project.yml` these are split into sdk=iphoneos* and sdk=iphonesimulator* conditional entries so that device builds link against the correct aarch64-apple-ios Rust archives without requiring xcodebuild overrides. xcodegen strips custom `Info.plist` keys (e.g., NIP-46 URL schemes) not declared in `project.yml`; after running `xcodegen generate`, `Info.plist` must be reverted. `xcodegen generate` also rewrites `project.pbxproj` with churned UUIDs/file ordering even when no real changes; agents must run `git checkout HEAD -- ios/Chirp/Chirp.xcodeproj/project.pbxproj` after running it. Running `xcodegen generate` in `apps/nmp-gallery/ios/` is required when Swift files are added to the project on disk but are not yet registered in the Xcode build sources. The `ActorCommand` enum ABI must match between `libnmp_signer_broker.a` and `libnmp_app_chirp.a`; stale broker builds with fewer enum variants cause crashes because the broker sends commands with old discriminant numbers that the actor misinterprets. `start_device_log_cap` is banned for use because it blocks and never returns. The nsec sign-in and key-package status sections were added to `SettingsHubView`; the groups tab (lock.shield.fill icon) was added to `RootShell`. All active iOS development lives in ios/Chirp/. NmpPulse was an iOS end-to-end validation app that was deleted and merged into Chirp on 2026-05-18; its smoke scenarios now live in ChirpTests/SmokeScenariosTests.swift gated behind NMP_SMOKE=1. The capability injection point for the production app is the ChirpCapabilities class (formerly NmpPulseCapabilities), contained in ChirpCapabilities.swift (formerly NmpPulseCapabilities.swift), which includes KeychainCapability for at-rest secret storage. iOS production code has 0 fatalError() calls, 2 try! force-unwraps in JSONSerialization, and 6 diagnostic print() statements tagged NMP_DIAG.

A pre-build Run Script phase in project.yml generates BuildInfo.generated.swift with git branch/commit and build timestamp; the phase uses basedOnDependencyAnalysis: false so it always runs. The BuildInfo.generated.swift placeholder is committed so xcodegen picks it up, and the generated values file should be added to .gitignore.

The app bundle is named Chirp7z.app (not Chirp.app) with bundle ID io.f7z.chirp (not com.example.Chirp). The justfile's run-ios recipe has the wrong bundle ID (com.example.Chirp) and hardcodes iPhone 17 while the booted device is iPhone 17 Pro.

<!-- citations: [^00480-1] [^00480-2] [^d27a4-2] [^f0034-1] [^1c093-14] [^30bf8-1] [^45fcf-2] [^45c5d-1] [^63dfc-1] [^e6b44-6] -->
