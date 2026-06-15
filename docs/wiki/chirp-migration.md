---
title: Chirp Migration
slug: chirp-migration
topic: chirp-migration
summary: The TUI Chirp shell uses NMP registry components for name (NostrProfileName), content (NostrContentView), avatar (NostrAvatar), and NIP-05 (NostrNip05Badge), ma
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
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
  - session:418d555f-8e77-4e56-8166-93d1fef9cfce
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
  - session:c9a794f6-6ad7-4ee9-a620-fc342fd495c3
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
---

# Chirp Migration

## TUI Shell

The TUI Chirp shell uses NMP registry components for name (NostrProfileName), content (NostrContentView), avatar (NostrAvatar), and NIP-05 (NostrNip05Badge), making it fully compliant. Chirp-tui must boot into the live home feed and respond to user input (`?` opens Help, `Esc` closes it, `/` opens command palette). ThreadScreen does not provide LocalProfileClaimer, so thread author names never trigger on-demand kind:0 fetch. KernelProfileHost must be keyed on remember(model) only, not remember(model, profiles), because profiles is a new Map on every snapshot tick, causing claim/release churn loops. profileHost must be removed from DisposableEffect keys in NostrAvatar and NostrProfileName — the host object identity changes on every snapshot, causing infinite release→re-claim loops. Chirp TUI must not keep an all-kinds raw event cache or build protocol hashtag filters in shell code; those belong in Rust projections. chirp-tui's runtime depends only on the native-gated C-ABI symbol nmp_app_ack_action_stage, not on any test-support-gated symbols, so its test-support feature must reside in [dev-dependencies], not runtime [dependencies], for both nmp-core and nmp-ffi. PR #1431 provides the fix for the chirp-tui freeze, scoped to Cargo.toml only and referencing issue #1430.

Repost/boosted-by attribution does not render a reposter name in Chirp (isRepost is a presence-bool only), so there is no unclaimed-republisher gap to fix.

<!-- citations: [^ab806-40] [^02745-95] [^da6b1-4] [^02745-76] [^02745-124] [^418d5-3] [^418d5-6] [^418d5-10] [^019ec-29] -->
## Android Shell

The Android Chirp avatar image-loading bug (authorPictureUrl decoded but never displayed, rendering only initials) was fixed by installing the NMP registry NostrAvatar component with Coil SubcomposeAsyncImage and building a KernelProfileHost claim/release adapter.

Android content rendering (NostrRichText → NostrContentView) is deferred as a separate migration (#984/F-CR-07) because Chirp's FlatBuffers-generated ContentTreeWire type collides with the registry's kotlinx.serialization nmp.content type.

The Chirp Android arm64-v8a APK with marmot and NIP-55 was built and placed at ~/Builds/chirp-debug-arm64.apk.

The gallery login-block Compose component is the canonical Android login UI; Chirp vendors the same file with only the package declaration changed, enforced by a VendorDriftGateTest. The Compose profile-component family is vendored under a byte-identical drift gate; any fix must edit both crates/nmp-cli/registry/compose/... and android/app/src/main/java/org/nmp/android/components/... identically.

The Android profile-claim churn bug (same class as chirp-web 4d1888f9a) is fixed by removing profileHost from DisposableEffect keys in NostrAvatar and NostrProfileName, and stabilizing KernelProfileHost via remember(model) + rememberUpdatedState(profiles).

<!-- citations: [^02745-96] [^da6b1-5] [^da6b1-6] [^da6b1-7] [^da6b1-42] [^02745-77] [^da6b1-79] [^02745-125] -->
## iOS Shell

iOS Chirp uses a 5-rung resolveAuthorLabel flicker fix that must be preserved in-app rather than moved into NostrProfileName, because it is multi-projection data resolution rather than rendering.

iOS Chirp swapped 12 ChirpAvatar call sites to NostrAvatar, author names to NostrProfileName, and the inline NIP-05 badge to NostrNip05Badge; content was already compliant via NostrContentView. The migration dropped the registry's bundled NostrIdenticon to avoid symbol collision with Chirp's existing grid NostrIdenticon in ContentTreeWire.swift; one NostrIdenticon remains (Chirp's).

The NostrAvatar Equatable guard (Fix 4) must land in the NMP Gallery registry component (apps/nmp-gallery/ios/NmpGallery/Registry/NostrAvatar.swift) first, then Chirp's customized copy, because the registry is the authoritative source. The Equatable conformance must include all rendered inputs (pubkey, url, colorHex, initials, size), not just (pubkey, url, colorHex), otherwise updates differing only in initials or size are wrongly suppressed. Late picture arrival for a url == nil (host-backed) NostrAvatar must still trigger a repaint after the Equatable guard is added; this correctness property must be verified, not assumed.

The iOS device archive for Chirp was built with IPHONEOS_DEPLOYMENT_TARGET=17.0 and --features marmot, producing a distribution-signed IPA (12 MB) at /tmp/Chirp-ipa/Chirp7z.ipa, signed with Apple Distribution SANITY ISLAND LLC team 456SHKPP26.

The Chirp iOS TestFlight upload requires an App Store Connect app record for io.f7z.chirp; Xcode auto-created an iOS Team Store Provisioning Profile during archive, confirming the bundle ID is registered, but the altool upload step requires the ASC issuer UUID.

The ChirpTests target fails to compile on master due to stale ProfileCard.npub/npubOffset references from ADR-0032/V-115 removal and a missing SwiftUI import — pre-existing, not introduced by the component migration.

profileHost must be removed from DisposableEffect keys in NostrAvatar and NostrProfileName — the host object identity changes on every snapshot, causing infinite release→re-claim loops.

<!-- citations: [^02745-97] [^da6b1-8] [^da6b1-9] [^da6b1-10] [^da6b1-43] [^c9a79-19] [^c9a79-27] -->
## Desktop and Web Shells

Chirp desktop (egui) cannot use registry desktop components because they are built with iced (framework mismatch); web (SolidJS) has no registry component target at all; both are blocked on a product decision for v1.

Desktop chirp-desktop silently drops signer_state, bunker_handshake, and nip46_onboarding projections (no find() call for any of their schema IDs in decode_snapshot_typed), leaving the bunker sign-in flow with no UI feedback. It must decode these typed sidecars so the Settings pane can show handshake progress instead of a static URI with no feedback.

Desktop chirp-desktop never calls nmp_app_ack_action_stage, causing action_stages entries to accumulate indefinitely until MAX_TRACKED_CORRELATIONS (1024) evicts old entries silently. It must call nmp_app_ack_action_stage for terminal action stages in the Outbox panel.

Desktop chirp-desktop leaves the nsec in three plain heap allocations (fs::read_to_string, KeyringResult, serde_json serialization) with no zeroization; the nsec is also persisted as a plaintext file at mode 0600 under ~/Library/Application Support/chirp-desktop/sessions/. It must wrap keyring secret intermediates in Zeroizing<String> to prevent nsec strings from persisting in plain heap allocations across multiple copies.

Desktop chirp-desktop double-rends every note card body on every frame (ui.label at line 1054 followed by note_body at line 1059) and contains dead effective_content code that JSON-parses kind:6 content the kernel already unwraps at projection time. It must remove the duplicate ui.label(text.as_ref()) call at app.rs:1054 that renders every note card body twice per frame alongside the note_body rich-text render.

Chirp Web must be simple in implementation terms — if it becomes complex, something is wrong in a lower layer that must be fixed at that layer. <!-- [^bf035-161] -->

Content rendering in the web uses ContentTreeWire (NFCT) bytes already present in every TimelineEventCard inside the NOFS feed projection — no new schema/kernel/drift work needed for content, only TypeScript decode and render. The kernel's ContentTreeWire (NFCT) bytes are already present in every TimelineEventCard inside the NOFS feed projection that the web decodes; the web was just ignoring them and rendering plaintext. <!-- [^bf035-162] -->

Quoted-event embed cards in Chirp require decoding the claimed_events (KCEV) projection, claiming referenced events, and wiring EventRefNode to render as a card rather than a raw nostr: link. <!-- [^bf035-163] -->

<!-- citations: [^da6b1-11] [^02745-24] [^02745-25] [^02745-26] [^02745-27] [^02745-79] [^da6b1-80] -->
## Cross-Platform Parity

Issue #1291 (Chirp feature-parity) is resolved as GO: full iOS+Android+desktop parity via thin-shell wiring only, filed as per-platform wiring tasks after #1283 and #980 land. Issue #1291 was filed for the cross-platform feature-parity gaps (Android react/follow/repost/zap buttons absent, iOS profile follow nil, profile-edit absent, NIP-65 publication) as owner product-scope decisions, not autonomous fixes, because Chirp is a thin proof app. <!-- [^02745-78] -->

## Registry Maintenance

When registry.json becomes stale after component edits, regenerate it via nmp export jsrepo --registry crates/nmp-cli/registry --output web/registry/public and commit the JSON. <!-- [^02745-80] -->

Always do the right thing on breaking changes — never hedge on migration scheduling; manually upgrade NMP consumer apps (podcast-player/hl/win-the-day) by git-rev bump each time. <!-- [^02745-123] -->
