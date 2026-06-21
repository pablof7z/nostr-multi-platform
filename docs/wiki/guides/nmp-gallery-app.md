---
title: NmpGallery App
slug: nmp-gallery-app
topic: component-registry
summary: NmpGallery must have both iOS (SwiftUI) and Android (Compose) versions, with TUI and Web versions planned for the future
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-06-18
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:c8c2902c-43a6-4b1c-8215-1732dc266895
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# NmpGallery App

## Platform & Architecture

NmpGallery must have both iOS (SwiftUI) and Android (Compose) versions, with TUI and Web versions planned for the future. NmpGallery uses the NMP kernel for relay connectivity (not custom WebSocket code) — the kernel handles relay connections, profile fetching, and data delivery automatically. The app must connect to real Nostr relays to fetch data — no hardcoded mock data for profile or content rendering. Gallery bootstrap seeds three relays (wss://purplepag.es, wss://relay.damus.io, wss://nos.lol) at startup so the kernel has routing targets even without a logged-in user. Android NmpGallery uses a JNI bridge with `nativeNextUpdate(handle, timeoutMs)` that blocks in Rust and returns JSON to a Kotlin coroutine loop, mirroring the existing Chirp Android pattern. The nmp-app-gallery Rust crate requires an `android-ffi` feature gate that enables JNI symbols and activates the `jni = 0.21` dependency. The `android.rs` JNI shim must be compiled under the `android-ffi` feature and exports 11 JNI entry points following the `Java_org_nmp_gallery_bridge_KernelBridge_*` naming pattern.

<!-- citations: [^53838-6] [^c8c29-1] -->
## Data Fetching & Authentication

The NMP kernel must retrieve real data (kind:0, kind:10002) for any pubkey via `nmp_app_claim_profile` even without a signed-in user — the application must work fetching data from relays without authentication. A standalone Rust validation example exists at `crates/nmp-app-template/examples/validate_claim_profile.rs` that proves `nmp_app_claim_profile` fetches kind:0 from purplepag.es in ~750ms without a signed-in user, exiting with code 0 on success. The gallery uses `nmp_app_open_author` (not `nmp_app_claim_profile`) to resolve demo profiles because `claim_profile` results don't surface on the snapshot wire for claim-only pubkeys; `open_author` populates `projections.author_view.profile`. Kernel snapshot profile data flows via `projections.author_view.profile` (a `ProfileCard` struct) only when `open_author` is called, not via `claim_profile`. The `ProfileCard` Rust struct has `pubkey`, `npub`, `display_name`, `picture_url`, `nip05`, `about`, `has_profile` fields but does NOT have `npub_short`. The `KernelBridge.kt` must expose an `openAuthor(pubkey: String)` public method backed by a `nativeOpenAuthor` external function, and the Android gallery's `nativeOpenAuthor` JNI method calls `nmp_ffi::nmp_app_open_author` to populate profile data via `projections.author_view`.

<!-- citations: [^53838-7] [^c8c29-2] -->
## Profile Rendering & Fallbacks

NmpGallery must never show a loading spinner for profile data — instead render a best-effort fallback immediately (identicon for avatar, truncated npub for name) and update reactively when kind:0 arrives from the relay. The `content-mention-chip` page must render 'Hey @pablof7z, how are you?' with the mention resolved from kernel kind:0, demonstrating that apps don't need to do anything special to make mention resolution work. <!-- [^53838-8] -->

## Crate API & Wire Format

The `nmp-app-gallery` Rust crate exports the standard `nmp_app_*` C-ABI symbols plus `nmp_app_gallery_register` (calling `register_defaults`) and `nmp_app_gallery_snapshot` (returning a minimal status envelope, not profile data). The `nmp_app_gallery_register` C-ABI function returns void (not void*); the header was fixed from the incorrect `void*` declaration to match the Rust signature. The `nmp_app_gallery_snapshot` C function returns a minimal JSON status envelope `{schema, alive, projections:{}}`, not profile data — it is a readiness probe only. Profile data flows to the gallery app via the push callback (`nmp_app_set_update_callback`), not via `nmp_app_gallery_snapshot` — the snapshot function is only a readiness/status probe. The kernel snapshot wire format is `{"t":"snapshot","v":<KernelSnapshot>}` — an envelope — not a bare object. Decoders must unwrap the envelope before reading projections. <!-- [^53838-9] -->

## iOS Type Deduplication

The `NostrIdenticon` enum is defined in both `ContentTreeWire.swift` (full version with `identiconView`) and `NostrAvatar.swift` (simple version). When both are in the same module, the full version from ContentTreeWire must be kept and the simpler version from NostrAvatar must be replaced. <!-- [^53838-10] -->

## Android Gallery Model & Profile Decoding

GalleryModel.kt must call `bridge.openAuthor(DEMO_PUBKEY)` instead of `bridge.claimProfile` to trigger profile loading. GalleryModel.kt must decode profile data from `projections.author_view.profile` in the kernel snapshot, not from `snapshot.profiles`. GalleryModel.kt must use safe casts (`as? JsonObject`) instead of `.jsonObject` extension property, because `.jsonObject` throws on `JsonNull`. ProfileWire.kt must make `npub` and `npubShort` optional with defaults, computing `npubShort` from `npub` when not provided.

Android WalletScreen must bind the Rust-computed `WalletStatus.is_connected` bool verbatim (matching iOS WalletView.swift) instead of deriving connectivity from the tone discriminant, so an errored wallet correctly shows as not-connected. (Previously: derived connectivity from the tone discriminant.)

<!-- citations: [^c8c29-3] [^11850-37] [^11850-77] -->
## Web Registry & Screenshots

Web registry user component screenshots follow the naming convention `<component>-kotlin-preview.png`. The web registry `user.ts` must reference screenshots for all 5 user Compose components: user-avatar, user-name, user-nip05, user-npub, user-card, each with a single `-kotlin-preview.png` entry. User component screenshots must be captured and merged as a PR before content component screenshots. <!-- [^c8c29-4] -->

## Merged PRs

PR #570 has been merged to master containing the JNI shim, fixed Android gallery app, and 5 user component Kotlin/Compose screenshots. PR #1530 has been merged, containing the Android WalletScreen fix to bind WalletStatus.is_connected verbatim instead of deriving from the tone discriminant.

<!-- citations: [^c8c29-5] [^11850-78] -->
