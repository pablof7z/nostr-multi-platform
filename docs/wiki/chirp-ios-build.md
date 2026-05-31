---
title: Chirp iOS Build & Xcode Configuration
slug: chirp-ios-build
summary: The iOS project uses DEVELOPMENT_TEAM 456SHKPP26 (SANITY ISLAND LLC) with CODE_SIGN_STYLE Automatic and a wildcard provisioning profile for device builds.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:f003440d-ee18-49d2-aa43-f2e806706008
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:30bf8c76-8be2-4e26-b22d-30ca86c37162
  - session:45c5d788-6be0-4b50-85da-52ee2538a65d
  - session:63dfcbb3-3ae0-48bb-9228-a494f85df203
  - session:0048057e-cb95-4da0-9f74-039a07dfc89f
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:485a5310-d073-41c9-b230-e6e77926a143
  - session:16ca6097-5734-4d49-8b9a-0a20d42a324b
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
---

# Chirp iOS Build & Xcode Configuration

## Code Signing

The iOS project uses DEVELOPMENT_TEAM 456SHKPP26 (SANITY ISLAND LLC) with CODE_SIGN_STYLE Automatic and a wildcard provisioning profile for device builds. [^582fc-10]



iOS Chirp ships a hardcoded 21,000 msat zap default to production. [^cd2b6-6]
## Project Generation

xcodegen is used to regenerate Chirp.xcodeproj from project.yml after adding new Swift source files or changing project settings; regeneration is also required when Swift files exist on disk but are missing from the project build sources. The updated project.pbxproj should be committed after xcodegen regeneration. New generated Swift files are included by running `xcodegen generate` rather than manually editing the pbxproj file. A symlink from the workspace target/ directory to ~/.cargo/target-shared/ is required for Xcode to find Cargo-built iOS libraries. The Chirp Xcode project links against Rust release builds, not dev builds; iOS device builds must run `cargo build --release --target aarch64-apple-ios -p nmp-app-chirp --features marmot` before the Xcode build when Rust code changes, because Xcode links the pre-built static library. Chirp's Xcode build links the nmp-app-chirp static lib (libnmp_app_chirp.a) to expose the modular timeline payload via FFI. Static libraries linked into the iOS app (libnmp_app_chirp.a, libnmp_signer_broker.a, libnmp_core.a) must all be rebuilt from the same nmp-core source to prevent ActorCommand enum ABI mismatches that cause crashes. The `LIBRARY_SEARCH_PATHS` in `project.yml` must use SDK-conditional settings (`sdk=iphoneos*` and `sdk=iphonesimulator*`) to ensure device builds link against the correct `aarch64-apple-ios` Rust archives. The iOS Rust build for nmp-app-chirp requires IPHONEOS_DEPLOYMENT_TARGET=17.0 to fix a ___chkstk_darwin linker error caused by building nmp-marmot's cdylib target against the iOS 26.5 SDK with the Rust target's default iOS 10.0 deployment floor; iOS does not support building cdylib (dynamic library) targets, which causes nmp-marmot's cdylib crate type to fail linking. The justfile should include a rust-ios-device recipe that sets the IPHONEOS_DEPLOYMENT_TARGET=17.0 environment variable alongside the existing rust-ios-sim recipe. Active iOS development lives in ios/Chirp/. iOS is the riskiest migration step because Xcode project files, xcodegen specs, and DerivedData paths all embed the current ios/Chirp/ root. The capability injection file formerly named NmpPulseCapabilities.swift is renamed to ChirpCapabilities.swift (or AppCapabilities.swift); the comment header drops all NmpPulse references, and all references to NmpPulseCapabilities are replaced with ChirpCapabilities. NMP_DIAG `print()` statements in iOS must be removed before release. The app bundle is named Chirp7z.app with bundle ID io.f7z.chirp.

<!-- citations: [^582fc-11] [^423f3-2] [^d27a4-4] [^f0034-1] [^f0034-2] [^d27a4-3] [^fe79b-2] [^1c093-3] [^30bf8-1] [^45c5d-1] [^63dfc-5] [^00480-1] [^16ca6-3] [^9a2c7-3] -->
## Simulator Build Workaround

iOS simulator builds require ENABLE_DEBUG_DYLIB=NO and FRAMEWORK_SEARCH_PATHS prepended with /tmp/LocalFrameworks to work around Xcode 26 beta SwiftUICore allowable-clients and UIUtilities linking errors. The Xcode 26 beta workarounds in project.yml are scoped to sdk=iphonesimulator* so they do not affect device builds. The /tmp/LocalFrameworks/ directory must contain patched SwiftUICore.tbd (with allowable-clients block removed) and UIUtilities.tbd stub before building the iOS simulator target. Chirp iOS uses the dedicated simulator with UUID `121F34F8-B41E-41F6-B788-2188D183BD97` named 'Use this for Chirp iOS' for running the app. NmpPulse's e2e kernel validation functionality lives in ChirpTests/SmokeScenariosTests.swift gated behind NMP_SMOKE=1.

<!-- citations: [^582fc-12] [^f0034-3] [^485a5-1] [^9a2c7-4] -->
## Build Info Generation

A pre-build Run Script phase in project.yml writes BuildInfo.generated.swift with the git branch, commit hash, and build timestamp. The script runs with basedOnDependencyAnalysis set to false so it always executes. A placeholder BuildInfo.generated.swift is committed so xcodegen includes it in the project, but BuildInfo.generated.swift is added to .gitignore so generated values are not stored in source control. The welcome screen displays the current git branch, commit hash, and build time at the bottom; these values are picked up and rendered automatically without manual updates. The build info footer uses .safeAreaInset(edge: .bottom) to pin the text above the home indicator without modifying the existing VStack layout. The build info text uses .secondary color for sufficient contrast on a white background. [^00480-2]
## See Also

