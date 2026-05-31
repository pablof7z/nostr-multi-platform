---
title: Android App Architecture & Build
slug: android-app-architecture
summary: The Android session creates and edits ONLY the new top-level android/ directory and may add a thin crates/nmp-android-ffi/ shim crate, without touching any othe
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:ad1d532e-a335-44fb-827e-a3f0318a3aae
  - session:e2d58641-a6c3-4f43-94c0-b018c8fbb893
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:16ca6097-5734-4d49-8b9a-0a20d42a324b
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Android App Architecture & Build

## Scope and Constraints

The Android session creates and edits ONLY the new top-level android/ directory (a monolithic multi-module Gradle project containing both Chirp and Gallery as subprojects, which must be split into independent Gradle builds when migrating to the apps/ tree) and may add the standalone workspace cdylib crate crates/nmp-android-ffi/, which bridges Kotlin to nmp-core via JNI, without touching any other directories. The Android app consumes the SAME Rust kernel the iOS app uses via raw C JSON FFI in crates/nmp-core/src/ffi/, which must be read but not modified. The app package is org.nmp.android with minimum SDK 26 (Android 8.0+). Both crates/nmp-android-ffi/ and the android/ Kotlin app tree must be tracked in git or deleted to prevent accidental loss.

<!-- citations: [^ad1d5-1] [^e2d58-1] [^57528-3] [^16ca6-1] -->
## Architecture

The Android app mirrors the iOS NmpPulse architecture with a KernelBridge (JNI → cargo-ndk built libnmp_core.so), a KernelModel observable, and Compose UI screens for Timeline + Diagnostics. The canonical FFI reference is docs/ffi-surface.md and the iOS NmpPulse/NmpPulse/Bridge/* provides the contract to mirror. The app label in the manifest is 'Chirp' (not 'NmpPulse'). nmp-core exposes FFI functions for Android via an 'android-ffi' Cargo feature that re-exports all kernel symbols through pub use at the crate root. nmp-android-ffi depends on nmp-core with the android-ffi feature enabled. JNI wrapper functions in nmp-android-ffi must call nmp-core kernel functions through Rust crate paths (e.g., nmp_core::nmp_app_new()) rather than extern "C" declarations, because extern "C" references are opaque to Rust's CGU compilation and leave symbols undefined in the .so. The KernelSymbolTable struct with #[used] containing function pointers does NOT force rlib archive member inclusion in PIC Android cdylib builds because function pointers in DATA sections create GOT dynamic relocations, not static archive-pulling relocations. The -Wl,-u linker flag approach does not resolve undefined kernel symbols because nmp-core's rlib is consumed at compile time into CGU files and is never passed to lld as an archive to search. Android currently has zero write capability — crates/nmp-android-ffi has no dispatch_action JNI symbol. nativeDispatchAction JNI is a prerequisite for all Android write parity. Read/navigation operations (openThread/openAuthor) are a separate concern from write parity. Android must implement dispatchAction, openThread, and openAuthor JNI symbols. KernelModel must expose the full write surface (DM, relay, zap, account, follow). Account operations (signInNsec, switchAccount, removeAccount) must call bespoke C-ABI JNI symbols (nativeSignInNsec, nativeSwitchAccount, nativeRemoveAccount) directly because no ActionModule is registered for those namespaces. createLocalAccount must route through dispatch_action. addRelay/removeRelay must have corresponding nativeAddRelay/nativeRemoveRelay JNI symbols and UI buttons must be unblocked. zapNote must use the nmp.nip57.zap namespace (not nmp.zap), the correct field name, and include recipientPubkey.

<!-- citations: [^ad1d5-2] [^e2d58-2] [^f3d8d-1] -->
## Native Build

Native libraries are built for arm64-v8a + x86_64 via `cargo ndk`. A Gradle module is wired to build the Android app. [^ad1d5-3]

## Doctrine Compliance

Doctrine D0–D8 (from docs/product-spec/overview-and-dx.md §1.5) must be honored: no business logic or cached state in Kotlin, no errors thrown across FFI (envelope/JSON only), and best-effort rendering (no spinner gates). [^ad1d5-4]

## Verification and Launch

The app must launch in an Android emulator, or exact emulator/SDK steps must be documented if the toolchain is absent, without faking a green result. The final deliverable requires android/ to build, the app to launch showing a live timeline through the kernel, and screenshots to be saved to docs/perf/android/. [^ad1d5-5]

## Commit Convention

Commit messages use the prefix `feat(android):`. [^ad1d5-6]

## UI Stack

The Android app uses Jetpack Compose with Material 3, Navigation Compose for typed navigation, Coil for async image loading, and kotlinx-serialization for JSON decoding. The login screen provides a Sign In button for nsec key entry and a Create Account button that generates a fresh keypair via nmp_app_create_new_account. MainActivity uses 6-tab navigation (Timeline / DMs / Relays / Account / Wallet / Diagnostics) with a NavHost supporting routes for profile/{pubkey}, thread/{eventId}, accounts, and diagnostics deep links. Required screens include SignInScreen, DmScreen, RelayScreen, ProfileScreen, and WalletScreen. SignInScreen must use the shared KernelBridge instance — creating its own KernelBridge produces a dead unstarted handle that prevents sign-in. DmScreen must call claimProfile and DmConversationListScreen must accept a model parameter so peer names load. The ComposeSheet component uses a ModalBottomSheet with a 280-character limit and reply support. PullToRefreshBox is not available in Material3 1.2.1 and must be replaced with a plain Box and LazyColumn. Author name must fall back through the profile merge chain; the model default in RootCardRow must not be nullable.

<!-- citations: [^e2d58-3] [^f3d8d-2] -->
## See Also

