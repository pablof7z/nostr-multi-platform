---
title: NMP App Integration
slug: nmp-app-integration
topic: nmp-app-integration
summary: The hl app fully embeds the NMP kernel via path deps (including nmp-ffi with the external-signer feature) and uses UniFFI (not JNI) for the FFI boundary, so no
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
  - session:019ec57a-fb01-7081-80c8-d7107f302049
---

# NMP App Integration

## Kernel Integration

The hl app fully embeds the NMP kernel via path deps (including nmp-ffi with the external-signer feature) and uses UniFFI (not JNI) for the FFI boundary, so no C-ABI symbols are added; all new NIP-55 methods flow through the existing UniFFI boundary. (Previously: the Android bridge was called out specifically.) The nmp-app-template crate no longer exists (renamed to nmp-defaults via ADR-0046); podcast-player's pin carried over as nmp-defaults.

<!-- citations: [^da6b1-24] [^da6b1-53] -->
## Android Frame Decoding

The podcast-player Android app has no Kotlin-side UpdateFrame/payload decoder; binary frames are decoded in Rust via nmp_app_podcast_decode_update_frame, so no Tier-3 Kotlin rebuild was needed for NIP-55. The nmp_free_string rename in v0.6.0 required podcast-player to adapt; the app was already past this rename on main when the v0.6.2 bump landed.

<!-- citations: [^da6b1-25] [^da6b1-54] -->
## Known Defects

The podcast-player has a pre-existing latent push-path bug: SnapshotCodec.decodeEnvelope decodes the envelope directly as PodcastSnapshot, but the wire never carried that shape; same defect class as NMP #1084. <!-- [^da6b1-26] -->


iOS `EmbedHost.swift` reimplements the Rust embed-projection resolver in Swift (a D0 violation), switching on raw Nostr kind integers, parsing kind:0 JSON via `JSONSerialization`, and heuristically detecting media URLs, duplicating logic that lives in `nmp-content/src/embed_projection/mod.rs`.

iOS `ThreadNoteRow` re-derives `isRepost` from the raw `kind: UInt32` integer instead of consuming the Rust-emitted `isRepost: Bool` typed field on `TimelineItem`.

iOS `RelaySeeding.swift` hardcodes relay URLs and parses JSON in Swift, while Android delegates to Rust via `nmp_chirp_config::chirp_default_relay_bootstrap()`; no C-ABI `nmp_app_seed_default_relays` symbol exists for iOS to call.

Desktop `app.rs` double-renders every note card body on every frame: line 1054 renders `ui.label(text.as_ref())` (plain text) and line 1059 immediately calls `note_body(ui, text.as_ref())` (rich re-tokenization), showing content twice at 60fps.

Desktop `app.rs`'s `effective_content` function is dead code: it attempts to JSON-parse kind:6 content, but the kernel projection already extracts the inner note body before the card reaches the shell.

Desktop `decode_snapshot_typed` silently discards `signer_state` (KSST), `bunker_handshake` (KBHS), and `nip46_onboarding` (KNBO) projections — there are no `find()` calls for any of their schema IDs, so bunker sign-in has no UI feedback (no progress, success, failure, or timeout states rendered).

Desktop never calls `nmp_app_ack_action_stage`, so action stage outbox entries accumulate indefinitely until exceeding `MAX_TRACKED_CORRELATIONS` (1024), bloating the `action_stages` sidecar on every snapshot tick.

Desktop keyring allocates the nsec secret in multiple plain heap `String`s (via `fs::read_to_string`, `KeyringResult`, `serde_json::to_string`, `CString::new`) with no zeroization; macOS Keychain is explicitly bypassed by design.

Desktop `follow_list` projection is decoded but silently unused — follow/unfollow are wired in the bridge but no follow button is rendered in the UI.

Desktop `DmConversationListScreen` double-collects `model.state` independently from its parent `DmScreen`, creating a minor consistency hazard where profiles and conversations can reflect different snapshot generations.

Desktop `ThreadScreen` does not provide `LocalProfileClaimer`, so thread author names never trigger on-demand kind:0 fetch (a functional gap, not a leak).

Android `KernelProfileHost.kt` keys `remember(model, profiles)` on a per-tick-fresh `profiles` map, recreating the host every snapshot tick, which causes `NostrAvatar` and `NostrProfileName` DisposableEffect to fire claim/release on every tick (the same churn bug as chirp-web commit 4d1888f9a). <!-- [^02745-8] -->

The podcast-player identity projection was gated only on an app-side rev counter that kernel-driven NIP-55 sign-in never advances; the fix dual-gates on the rev counter AND the kernel active-account hex. <!-- [^da6b1-55] -->
## Shared Component Vending

The Android login-block Compose component in nmp-gallery is the canonical source; Chirp vendors an identical copy with only the package declaration changed, enforced by a VendorDriftGateTest. <!-- [^da6b1-27] -->

## CI Pipeline Fixes

The podcast-player TestFlight pipeline had 40 consecutive failures because CI never built the Rust core for the simulator architecture, causing undefined symbol _nmp_free_string link errors; fixed by adding cargo build --target aarch64-apple-ios-sim to the test script (PR #429).

The hl repo's Xcode Cloud build was blocked by two issues: PR #4 fixed the nip55SignerPackage enum exhaustiveness in two switch statements (verified archive succeeds), and a shared Xcode scheme must be committed (added to project.yml so xcodegen regenerates it as shared). <!-- [^da6b1-28] -->


PR #1295 fixes the desktop double-render by removing `ui.label(text.as_ref())` and keeping only `note_body`, moves click-to-thread onto the scope response, and deletes the dead `effective_content` function.

PR #1295 promotes three `#[cfg(test)]` decode functions (`signer_state`, `bunker_handshake`, `nip46_onboarding`) to `pub`, adds sidecar decode blocks in `snapshot_decode.rs`, and updates the Settings pane to render live handshake progress.

PR #1295 adds `ack_action_stage` to the desktop `AppRuntime` bridge, calling it for every terminal-stage row in the Outbox panel.

PR #1295 wraps `read_to_string` result and JSON intermediates in `Zeroizing<String>` in keyring.rs to prevent nsec secrets from remaining in plain heap memory.

PR #1302 fixes the Android claim-churn by keying `rememberKernelProfileHost` on `model` only, threading the latest profiles map through a `profilesProvider` lambda backed by `rememberUpdatedState`, and removes `profileHost` from the DisposableEffect key lists in `NostrAvatar` and `NostrProfileName`.

PR #1285 splits `KernelBridge.swift` (~1300 LOC of standalone Decodable DTOs) into four cohesive type files (`KernelUpdateTypes`, `KernelSnapshotTypes`, `KernelActionTypes`, `KernelSignerTypes`), reducing KernelBridge from 2172 to 889 lines without bumping baselines. <!-- [^02745-9] -->
## Test & Debug Seams

NMP_TEST_RELAYS (iOS env var) and nmp.test_relays/nmp.test_nsec (Android debug intent extras) inject deterministic relay and identity overrides for E2E testing; no shell policy decisions are made with these strings. On iOS, KernelModel.swift reads NMP_TEST_RELAYS and replaces default relay seeding when present, mirroring the NMP_TEST_NSEC pattern. On Android, the NMP_TEST_RELAYS seam passes relay JSON as a debug-only intent extra, forwarded verbatim to the Rust bridge which uses it instead of hardcoded defaults — no Kotlin policy.

<!-- citations: [^78c8e-28] [^78c8e-54] [^78c8e-71] [^78c8e-88] [^78c8e-108] -->
## Cross-Platform Vending & FFI Readiness

The Compose profile components (NostrAvatar, NostrProfileName, KernelProfileHost) are vendored under a byte-identical drift gate — any fix must edit both `crates/nmp-cli/registry/compose/...` and `android/app/src/main/java/org/nmp/android/components/...`. Android has zero EmbedHost resolver duplication today (its `EmbedEntry` is pre-resolved in Rust), so landing the #1283 nmp-ffi sidecar now means Android writes a decode-only path from day one and the duplication never spreads. The nmp-android-ffi standalone Cargo.lock must not contain apple-native-keyring-store references after the hard break; it must contain base64 under nmp-marmot deps.

<!-- citations: [^02745-10] [^78c8e-109] -->
## Platform Constraints

Desktop (egui) and web (SolidJS) are framework-blocked for NMP component reuse — desktop because registry components are iced (not egui), web because there is no web registry target. The Chirp Android APK unconditionally builds with `--features marmot` (MLS), confirmed at `android/app/build.gradle.kts:79`. <!-- [^da6b1-56] -->


A feature belongs in an NMP crate when it is a general building block that any Nostr app could use directly; app-specific logic belongs in the app's own Rust crates under apps/<app>/. The line for crate placement is not protocol vs. product but generic Nostr building block vs. this app's proprietary domain. <!-- [^019ec-18] -->
## MLS Key Package Autopublish

All local-key sign-in paths (nsec sign-in, account creation, keyring restore) set the pending_mls_autopublish flag, and the autopublish tail is hoisted into the shared register_with_keys so every register path publishes a key package. The set_pending_mls_autopublish setter is pub(crate) and centralized into NmpApp::add_signer (D4 single-writer); tests exercise the real nmp_app_signin_nsec entry point. <!-- [^78c8e-72] -->

## Interest Enforcement

Interest declaration must be enforced at the nmp-defaults builder layer (mandatory for apps), while the kernel primitive remains permissive (legitimate for test/embedded Rust callers). <!-- [^78c8e-110] -->

Interest declaration must be enforced at the nmp-defaults builder layer (mandatory for apps), while the kernel primitive remains permissive (legitimate for test/embedded Rust callers). Malicious/untrusted-hosting security doctrine is not needed at the NMP level because the app is already trusted by the user; if napplet/runtime hosting is ever built, the security layer goes at that level. <!-- [^bf035-166] -->
