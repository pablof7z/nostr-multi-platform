---
title: Cross-Platform Architecture
slug: cross-platform-architecture
summary: "Cross-platform architecture: C-ABI funnel, declared observed projections, Capabilities injection, and worktree development workflow"
tags:
  - architecture
  - c-abi
  - ffi
  - kernel
  - observer
  - capabilities
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
---

# Cross-Platform Architecture

> Cross-platform architecture: C-ABI funnel, declared observed projections, Capabilities injection, and worktree development workflow

## C-ABI funnel

All three platforms (iOS, TUI, Android) funnel through the same nmp_app_create_new_account C-ABI symbol in nmp-ffi. This ensures identical business logic execution regardless of platform. The Android path is: KernelBridge.nativeCreateLocalAccount → nmp_app_create_new_account (nmp-ffi) → ActorCommand::CreateAccount → create_account() in identity.rs. [^855be-13]


The Android app is built and launched on an emulator via a Haiku agent. The build uses ./gradlew :app:installDebug, which includes cross-compiling the Rust JNI shim (nmp-android-ffi) via cargo-ndk. The emulator is the nmp_test AVD running Android 14 (arm64-v8a) with a visible GUI window — not headless. The app launches MainActivity and displays the Timeline screen. [^855be-19]
## Declared observed projections

The NOFS engine is fed through declared observed projections, not a public all-event observer. The composition root declares the event shapes the feed needs before it receives events; replay and future delivery stay scoped to those shapes. The EventGate check remains inside ingest() to intercept non-feed-eligible events before any state mutation. The profile_detector runs after the EventGate for kind:0 events. [^855be-14]

## Capabilities pattern

The RootIndexedFeed uses a Capabilities struct with caller-supplied predicates (profile_detector, event_gate, event_lookup) that are injected at construction time. This keeps the engine generic while allowing the composition root to supply platform- or protocol-specific behavior. The EventGate follows the same pattern as the pre-existing profile_detector. [^855be-15]

## Worktree-based development

Changes to the nmp-feed and nmp-nip01 crates must be made in an isolated worktree via an Agent with isolation: 'worktree' and submitted via PR. Direct edits to the main checkout are reset by a heartbeat process that continuously reverts uncommitted changes. A linter may also revert files independently of the heartbeat.

PR #786 bundled both the NOFS EventGate kind filter and the Android Diagnostics LazyColumn key fix. It was merged to master as commit 59874e5c. After merge, local master is synced with origin/master. [^855be-18]

<!-- citations: [^855be-16] [^855be-17] -->
## See Also
- [[nofs-op-centric-feed|NOFS OP-Centric Feed Engine]] — related guide
- [[diagnostics-screen|Android Diagnostics Screen]] — related guide
- [[account-creation-autofollow|Account Creation & Autofollow]] — related guide
