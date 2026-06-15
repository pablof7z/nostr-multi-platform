---
title: MLS Architecture
slug: mls-architecture
topic: mls
summary: Chirp MLS logic is owned by Rust; iOS and Android shells contain zero protocol/crypto/ratchet logic (only ADR-0032 display formatting)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
---

# MLS Architecture

## Architecture

Chirp MLS logic is owned by Rust; iOS and Android shells contain zero protocol/crypto/ratchet logic (only ADR-0032 display formatting). Shell code (e.g. Chirp TUI) must not keep raw all-kinds event caches or construct protocol filters; raw event caching and hashtag filter construction belong in Rust projections, not the UI shell. MLS cross-platform is conditionally operational on current master: both iOS and Android have complete create/invite/send/receive wired in UI, real durable key storage (Keychain/Keystore), and the marmot feature compiled into shipping builds; the only operational requirements are building iOS for device with --features marmot if not using simulator, and pointing both apps at common relays. MLS group messages are published under ephemeral per-message keys (a metadata-privacy property of MLS-over-Nostr), so sender attribution is decrypted from inside the MLS payload, not visible on the wire. The iOS justfile only has simulator build targets; building for a physical iPhone device requires passing --features marmot to the aarch64-apple-ios target and setting IPHONEOS_DEPLOYMENT_TARGET=17.0 (to resolve ___chkstk_darwin linkage against the Xcode 26 SDK). The Android APK unconditionally includes --features marmot in the cargo ndk build (build.gradle.kts:79), so the shipping APK always includes MLS.

<!-- citations: [^78c8e-1] [^da6b1-16] [^78c8e-24] [^78c8e-53] [^78c8e-69] [^da6b1-69] [^019ec-17] -->
## Proven Integration

Bidirectional cross-platform MLS messaging was proven live on real devices (iOS simulator ↔ Android emulator over relay.primal.net): iOS creates a group, Android receives the welcome invite and joins, both send and receive encrypted messages with correct sender attribution, and history persists across force-restart. Post-restart, live group messages arrive without a nudge.

<!-- citations: [^78c8e-2] [^78c8e-27] [^78c8e-70] -->
## Key Management

The unified keyring capability uses a single wire shape {op, account_id, secret} for MLS DB key storage; iOS implements it via KeychainCapability and Android via KeystoreKeyringCapability (AES-256-GCM hardware-backed), with in-memory mock degradation surfacing keyring_unavailable=true in the snapshot. The MLS DB key rides the host keyring capability on every platform via CapabilityCredentialStore; apple-native-keyring-store is deleted. The Android MLS DB key was previously stored in an in-memory mock keyring (lost on every restart), now replaced by the Keystore-backed capability store shared across both platforms via the nmp.keyring.capability socket — one secure-storage stack, no per-platform special-casing. Key-package autopublish is hoisted into the shared register_with_keys tail; all local-key sign-in paths set the pending_mls_autopublish flag.

<!-- citations: [^78c8e-3] [^da6b1-17] [^78c8e-25] -->
## Interest and Relay Subscriptions

PushInterest and EnsureInterest in nmp-core never served store-first cache events, causing kind-parsers (including MarmotIngestParser) to be blind to store-resident events on every session after the first — fixed by extracting a single enqueue_interest_cache_serve choke point called from all three install paths. After restart, register_with_keys re-pushed the gift-wrap inbox interest but never re-subscribed per-group kind:445 message feeds for already-joined groups, so live group messages never arrived — fixed by resubscribe_all_groups enumerating stored groups and routing them through the existing cache_group_relays choke point. <!-- [^78c8e-4] -->

## Group Operations and Error Handling

Marmot create_group/invite returns key_package_unavailable as a terminal failure with no retry; the deferred-completion fix parks ops when KPs are missing and retries them on KP ingest arrival, with expiry on snapshot/ingest edges. iOS MarmotOpResult.submitted discarded the correlation_id and the sheet dismissed on submission, hiding failures from the user — shells now keep the correlationId and stay open until a terminal verdict. <!-- [^78c8e-5] -->

## NMMS FlatBuffers Schema

The NMMS FlatBuffers schema version was bumped from 1 to 2, adding PendingOpRow (with op, display_label, missing_count, age_secs) and LastOpError table (op, reason, at_secs, correlation_id) — generated Rust/Swift/Kotlin bindings were regenerated with flatc 25.2.10 for Kotlin and 25.12.19 for Rust+Swift. <!-- [^78c8e-6] -->

## Android Kernel Model Refactor

Android KernelModel was 572 LOC (over the 500 hard cap) and used hand-rolled escapeJson for Marmot action envelopes — split into MarmotActions.kt and MarmotActionEnvelopes.kt with typed @Serializable DTOs encoded with chirpActionJson (replacing escapeJson), and added leave/invite/remove/clear_pending ops mirroring the iOS MarmotBridge surface.

<!-- citations: [^78c8e-7] [^78c8e-26] -->
## JNI and Build Correctness

The capability callback trampoline for non-external_signer namespaces uses JNI with_local_frame to reclaim local refs on every dispatch, preventing local-ref-table overflow on permanently-attached threads. Android --features marmot build was broken (missing libsqlite3-sys vendored OpenSSL dep, zeroize std feature, and missing jstring_to_cstring import) and uncaught by CI — fixed with a cargo check --features marmot CI gate in android-ffi's standalone workspace. NMP_TEST_RELAYS and Android debug intent extras (nmp.test_nsec, nmp.test_relays) are E2E test seams that override relay defaults without placing policy decisions in the shell — Kotlin only ferries strings, Rust owns the decision. <!-- [^78c8e-8] -->

## Debug Performance Note

The Debug build on the physical iPhone is unoptimized (dev Rust profile + Debug Swift), which makes the NMP core substantially slower than a release build — crypto, FlatBuffers, and the 4Hz snapshot path are all hot and unoptimized. <!-- [^78c8e-9] -->
