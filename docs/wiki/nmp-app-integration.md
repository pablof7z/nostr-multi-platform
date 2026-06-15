---
title: NMP App Integration
slug: nmp-app-integration
topic: nmp-app-integration
summary: All four consumer apps on the user's iPhone (Chirp, Highlighter, tenex-off, podcast-player) were rebuilt and installed with nmp-v0.8.0; win-the-day was confirme
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
  - session:019ec57a-fb01-7081-80c8-d7107f302049
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:418d555f-8e77-4e56-8166-93d1fef9cfce
  - session:286c6f24-af4b-4e59-b72f-ed72e8b9d781
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
---

# NMP App Integration

## Kernel Integration

All four consumer apps on the user's iPhone (Chirp, Highlighter, tenex-off, podcast-player) were rebuilt and installed with nmp-v0.8.0; win-the-day was confirmed to not consume NMP at all and required no update. The hl app fully embeds the NMP kernel via path deps (including nmp-ffi with the external-signer feature) and uses UniFFI (not JNI) for the FFI boundary, so no C-ABI symbols are added; all new NIP-55 methods flow through the existing UniFFI boundary. (Previously: the Android bridge was called out specifically.) The nmp-app-template crate no longer exists (renamed to nmp-defaults via ADR-0046); podcast-player's pin carried over as nmp-defaults. Parked crates that are excluded from the workspace must have an empty `[workspace]` table and explicit (non-inherited) `package` fields so they remain standalone-buildable as external path dependencies. The final pin for external consumers is nmp-v0.7.1 (92fdfca327a782b82ee999a414190d39265b8243), which includes the parked-crate standalone-buildability fix (#1424) on top of v0.7.0. nmp-v0.7.1 fixes the parked-crate defect by de-inheriting workspace fields in nmp-blossom and nmp-nip60, making them standalone-buildable as external path-deps. podcast-player adopts the 0.7.2 git-dep model (all 6 NMP crates pinned to 45ac8c3e4/0.7.2, the vendor/nmp-blossom directory and its [patch]/workspace-member entries are deleted, nmp-blossom resolves as an ordinary git dep with a single nmp-core version); it builds clean against nmp-v0.7.1 and now 0.7.2; no structural migration was needed. hl path-tracks the NMP monorepo via local path deps and builds green against 0.7.2 with nmp-blossom resolving as a normal workspace member; no hl code change is needed; these local path deps must be converted to explicit git-rev pins at the v0.8.0 commit to ensure the build actually contains the profile-resolution fix rather than building against the stale main checkout. win-the-day (RockingLife) has zero dependency on NMP — it uses a hand-rolled pure-Swift Nostr implementation with no Cargo.toml and no nmp_app_* FFI symbols — and requires no v0.8.0 upgrade. Consumer apps (podcast-player, hl, tenex-off) now track the NMP master branch via GitHub git deps rather than pinned revs, so they automatically pick up the latest NMP without re-pinning.

<!-- citations: [^2e544-377] [^da6b1-24] [^da6b1-53] [^2e544-415] [^2e544-434] [^2e544-467] [^ab806-189] [^ab806-253] [^ab806-265] [^ab806-278] -->
## Android Frame Decoding

The podcast-player Android app has no Kotlin-side UpdateFrame/payload decoder; binary frames are decoded in Rust via nmp_app_podcast_decode_update_frame, so no Tier-3 Kotlin rebuild was needed for NIP-55. The nmp_free_string rename in v0.6.0 required podcast-player to adapt; the app was already past this rename on main when the v0.6.2 bump landed.

<!-- citations: [^da6b1-25] [^da6b1-54] -->
## Known Defects

The podcast-player has a pre-existing latent push-path bug: SnapshotCodec.decodeEnvelope decodes the envelope directly as PodcastSnapshot, but the wire never carried that shape; same defect class as NMP #1084. iOS `EmbedHost.swift` reimplements the Rust embed-projection resolver in Swift (a D0 violation), switching on raw Nostr kind integers, parsing kind:0 JSON via `JSONSerialization`, and heuristically detecting media URLs, duplicating logic that lives in `nmp-content/src/embed_projection/mod.rs`. iOS `ThreadNoteRow` re-derives `isRepost` from the raw `kind: UInt32` integer instead of consuming the Rust-emitted `isRepost: Bool` typed field on `TimelineItem`. iOS `RelaySeeding.swift` hardcodes relay URLs and parses JSON in Swift, while Android delegates to Rust via `nmp_chirp_config::chirp_default_relay_bootstrap()`; no C-ABI `nmp_app_seed_default_relays` symbol exists for iOS to call. Desktop `app.rs` double-renders every note card body on every frame: line 1054 renders `ui.label(text.as_ref())` (plain text) and line 1059 immediately calls `note_body(ui, text.as_ref())` (rich re-tokenization), showing content twice at 60fps. Desktop `app.rs`'s `effective_content` function is dead code: it attempts to JSON-parse kind:6 content, but the kernel projection already extracts the inner note body before the card reaches the shell. Desktop `decode_snapshot_typed` silently discards `signer_state` (KSST), `bunker_handshake` (KBHS), and `nip46_onboarding` (KNBO) projections — there are no `find()` calls for any of their schema IDs, so bunker sign-in has no UI feedback (no progress, success, failure, or timeout states rendered). Desktop never calls `nmp_app_ack_action_stage`, so action stage outbox entries accumulate indefinitely until exceeding `MAX_TRACKED_CORRELATIONS` (1024), bloating the `action_stages` sidecar on every snapshot tick. Desktop keyring allocates the nsec secret in multiple plain heap `String`s (via `fs::read_to_string`, `KeyringResult`, `serde_json::to_string`, `CString::new`) with no zeroization; macOS Keychain is explicitly bypassed by design. Desktop `follow_list` projection is decoded but silently unused — follow/unfollow are wired in the bridge but no follow button is rendered in the UI. Desktop `DmConversationListScreen` double-collects `model.state` independently from its parent `DmScreen`, creating a minor consistency hazard where profiles and conversations can reflect different snapshot generations. Desktop `ThreadScreen` does not provide `LocalProfileClaimer`, so thread author names never trigger on-demand kind:0 fetch (a functional gap, not a leak). Android `KernelProfileHost.kt` keys `remember(model, profiles)` on a per-tick-fresh `profiles` map, recreating the host every snapshot tick, which causes `NostrAvatar` and `NostrProfileName` DisposableEffect to fire claim/release on every tick (the same churn bug as chirp-web commit 4d1888f9a). The podcast-player identity projection was gated only on an app-side rev counter that kernel-driven NIP-55 sign-in never advances; the fix dual-gates on the rev counter AND the kernel active-account hex. nmp-blossom is a v1 workspace member, not a parked dead island; it is CI-built and tested as part of the workspace. nmp-feedback is pinned at 0.7.2 (857dedf45be721d748bf4ed55a76144ba89018b9) with nmp-core/nmp-ffi re-pinned to 0.7.2. (Previously: nmp-feedback had a hard version constraint (nmp-core ^0.6.2) that rejected the 0.7.x breaking release; it was bumped to 0.7.2 and merged as pablof7z/nmp-feedback#1.) win-the-day (RockingLife) has zero dependency on NMP — it uses a hand-rolled pure-Swift Nostr implementation with no Cargo.toml and no nmp_app_* FFI symbols — and requires no v0.8.0 upgrade.

<!-- citations: [^da6b1-26] [^02745-8] [^da6b1-55] [^2e544-416] [^2e544-435] [^2e544-468] [^ab806-190] [^ab806-266] -->
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

The gallery's curated minimal nmp/transport subset must not be force-regenerated; only android/app/src/main/java/nmp/ gets the full regen. <!-- [^78c8e-455] -->

<!-- citations: [^02745-10] [^78c8e-109] -->
## Platform Constraints

NMP UI components should be used whenever possible; improvements to NMP UI components are preferred over app-specific workarounds. Desktop (egui) and web (SolidJS) are framework-blocked for NMP component reuse — desktop because registry components are iced (not egui), web because there is no web registry target. The Chirp Android APK unconditionally builds with `--features marmot` (MLS), confirmed at `android/app/build.gradle.kts:79`. The `test-support` feature on `nmp-core` and `nmp-ffi` must be declared in `[dev-dependencies]`, not runtime `[dependencies]`, for the `chirp-tui` crate.
A feature belongs in an NMP crate when it is a general building block that any Nostr application could use directly; the test is 'would this crate be useful to a completely different Nostr app?' App Rust crates (apps/<app>/) hold the Rust side of features specific to that application's domain that would not generalize to other Nostr apps; NMP does not accumulate app-specific logic. The line between NMP crates and app crates is generic Nostr building block vs. this app's proprietary domain, not protocol vs. product.

<!-- citations: [^da6b1-56] [^019ec-18] [^418d5-12] [^286c6-5] [^019ec-52] -->
## MLS Key Package Autopublish

All local-key sign-in paths (nsec sign-in, account creation, keyring restore) set the pending_mls_autopublish flag, and the autopublish tail is hoisted into the shared register_with_keys so every register path publishes a key package. The set_pending_mls_autopublish setter is pub(crate) and centralized into NmpApp::add_signer (D4 single-writer); tests exercise the real nmp_app_signin_nsec entry point. <!-- [^78c8e-72] -->

## Interest Enforcement

Interest declaration must be enforced at the nmp-defaults builder layer (mandatory for apps), while the kernel primitive remains permissive (legitimate for test/embedded Rust callers). <!-- [^78c8e-110] -->

Interest declaration must be enforced at the nmp-defaults builder layer (mandatory for apps), while the kernel primitive remains permissive (legitimate for test/embedded Rust callers). Malicious/untrusted-hosting security doctrine is not needed at the NMP level because the app is already trusted by the user; if napplet/runtime hosting is ever built, the security layer goes at that level. <!-- [^bf035-166] -->
