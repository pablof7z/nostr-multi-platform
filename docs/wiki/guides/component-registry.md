---
title: Component Registry
slug: component-registry
topic: component-registry
summary: The NMP component registry uses a shadcn/jsrepo-style model where `nmp add component` copies source files into the app, the app owns and edits them locally, and
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-26
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:e7a1d168-3c58-4438-a544-aa645850c388
  - session:f2fd46d3-1cbd-4f80-9469-0d8137d75478
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
---

# Component Registry

## Registry Model

The NMP component registry uses a shadcn/jsrepo-style model where `nmp add component` copies source files into the app, the app owns and edits them locally, and `nmp update component` performs conflict-safe updates using SHA-256 baselines stored in `nmp.components.lock`. The registry lock file (`nmp.components.lock`) uses hand-written TOML output (only `Deserialize` derived) for predictable formatting. The registry data model uses a per-component `platforms` map with Platform keys ('swift', 'kotlin') instead of separate per-platform sidebar entries. `nmp add component` only rejects the explicit target component if already installed; already-installed transitive dependencies are skipped silently rather than causing an error. `nmp update component` always advances the component-level version to the registry revision, even when conflicts exist; per-file `source_sha256` baselines encode divergence rather than the component version. A missing on-disk file during `nmp update component` counts as a conflict, not silently overwritten. The conflict output from `nmp update component` goes to stdout via `println!`, not stderr.

The NMP component registry catalogs 14 distinct component slugs across 3 categories (8 content, 5 user, 1 relay). The registry contains a total of 6,705 lines of source code across SwiftUI (2,673), Compose (2,285), and TUI (1,747). No component in the registry is flagged as inFlight; all listed implementations carry status stable. <!-- [^e7a1d-1] -->

The production deployment workflow for the registry is `vercel build --prod && vercel deploy --prebuilt --prod` (build locally then upload prebuilt output). Root .vercel/project.json points to the nmp-registry project. Root .vercelignore is configured for the registry (not chirp). Root vercel.json points at web/registry. <!-- [^f2fd4-1] -->

The nmp binary is in crates/nmp-cli/ and ships init, gen, add, update subcommands. nmp init creates a Rust workspace only (Cargo.toml, nmp.toml, app-core crate); full multi-platform starter (iOS/Android) is M16 planned. <!-- [^56d21-1] -->

<!-- citations: [^45258-2] [^45258-3] [^45258-4] [^45258-5] [^45258-6] [^53838-2] -->
## Registry Consistency Tests

The drift test `committed_registry_json_matches_generated_output` requires that any modification to `registry.toml` or registry source files must be accompanied by regenerating `web/registry/public/registry.json` via `nmp export jsrepo`. The `web/registry/src/registry.ts` must include matching `installId` entries for every component in `registry.toml`, enforced by the `web_registry_install_metadata_mirrors_cli_manifest` test. <!-- [^45258-7] -->

## M16 Component Registry Plan

The 7-step M16 component registry plan steps are: (1) Land PR #503, (2) Build `nmp update component`, (3) Freeze ContentTreeWire fixtures, (4) Replace tiny SwiftUI kit with real iOS renderer, (5) Build Android Compose parity, (6) Adopt in Chirp, (7) jsrepo export. <!-- [^45258-8] -->


The legacy `android/gallery/` module (pre-registry bundle viewer) must be deleted when merging PR #556 to avoid `applicationId` collision with the new `apps/nmp-gallery/android/`. <!-- [^53838-3] -->
## Registry Component Organization

The registry site organizes components by section (Content, User) with a platform switcher (Swift, Kotlin, TUI, Web) per component page, not by platform groups in the sidebar. TUI and Web platform tabs show a 'soon' badge and are disabled since no components are built for those platforms yet. The 'compose-' prefix on component slugs was eliminated in favor of the platform-switcher design where 'Kotlin' is the user-facing label and Jetpack Compose is the implementation detail. The `user-core` component was renamed to `user-avatar` because bundling a non-visual wire type (ProfileWire) with a visual component (NostrAvatar) was architecturally inconsistent for a registry entry. (Previously: `user-core`.) ProfileWire bundles into `user-avatar` since the wire type is always needed to construct an avatar, mirroring NDK's approach of having no separate 'core' for user components. Screenshot naming convention is `{slug}-{platform}-preview.png` (e.g., `user-avatar-swift-preview.png`, `content-core-kotlin-preview.png`). Screenshots must be taken using the NmpGallery iOS and Android apps (not ad-hoc preview apps), and added to `web/registry/public/screenshots/` for the website. Registry screenshots must be full-screen iPhone Simulator captures (e.g., 1206×2622), not cropped component slices, so they display properly inside the device mockup frame. The `DeviceMockup` CSS wrapper must be kept around screenshots (not removed), and screenshots should use `object-fit: contain` with a light background (#f2f2f7) so full-device images display correctly without zooming or cropping.

<!-- citations: [^45258-9] [^53838-1] -->

## Registry Platform Coverage

The web platform has zero components implemented (no HTML/TSX/Vue/Svelte source files exist in the registry). Compose is missing implementations for content-minimal, login-block, and relay-list. TUI is missing implementations for login-block and relay-list, and has no relay-list implementation on disk at all. Six TUI content components (content-core, content-minimal, content-mention-chip, content-media-grid, content-quote-card, content-view) exist as source files on disk (~1,398 LOC total) and are declared in registry.toml, but are not imported into the web registry TS files. User components (user-avatar, user-name, user-nip05, user-npub, user-card) are the only category with full tri-platform coverage (SwiftUI + Compose + TUI). <!-- [^e7a1d-2] -->

## Chirp iOS Registry Adoption

Chirp iOS uses registry-equivalent content components that were copied into the tree and have drifted from the canonical registry sources (confirmed by diff on NostrContentRenderer.swift and NostrContentView.swift). Chirp iOS does not use any registry user-profile components, instead using custom inline implementations: ChirpAvatar (initials + hex color, no identicon), inline Text(authorDisplayLabel), inline NoteRowView author header, and no NIP-05 or npub rendering. Chirp iOS uses custom login-block and relay-list implementations (OnboardingView+Components.swift, RelaySettingsView.swift, DiagnosticsView.swift) rather than registry components. Chirp iOS NoteContentView is a custom wrapper around NostrContentView + NostrContentRenderer with Chirp-specific routing and image viewer, duplicating registry content-view + content-core. Chirp iOS NoteRowView author header duplicates registry user-card, and NoteActionsRow is a candidate for a new registry content-actions component. <!-- [^e7a1d-3] -->

## Android Registry Adoption

A Chirp Android app directory does not exist in apps/chirp/ (which contains chirp-tui and chirp-repl only); the Android audit covers the main Android app at android/app/. The main Android app uses zero registry components; it uses custom monolithic replacements including NostrRichText (297 LOC replacing content-core + content-view), MediaViews (144 LOC replacing content-media-grid), inline Avatar (replacing user-avatar), inline display name (replacing user-name), inline Row (replacing user-card), and inline RelayRow (replacing relay-list). The gallery Android app (apps/nmp-gallery/android/) copies all registry Compose components plus gallery-quality additions (Identicon.kt and MentionChip.kt) that should be upstreamed or adopted into the main app. <!-- [^e7a1d-4] -->

## Cross-Platform Identicon Parity

The registry identicon implementations (SwiftUI NostrIdenticonBox with palette + initials, Compose 6-color palette with first-2-char initials) do NOT match the gallery's 5×5 symmetric block canvas algorithm, creating a cross-platform visual parity gap. P0 priority is to unify all platforms on the 5×5 symmetric identicon algorithm, using the gallery's Identicon.kt as the reference and porting it to registry SwiftUI + Compose. <!-- [^e7a1d-5] -->

## Adoption Priorities

P1 priority for iOS is to reconcile drifted content components by running a full diff and either re-aligning Chirp copies to registry canonicals or deleting them and reinstalling fresh via nmp add component. P2 priority for iOS is to adopt registry user-profile components (user-avatar replacing ChirpAvatar, user-name replacing inline Text, user-card replacing NoteRowView author header, user-nip05 and user-npub added to ProfileView). P3 priority for the Android app is to adopt compose/content-core, compose/content-view (replacing NostrRichText.kt), and compose/content-media-grid (replacing MediaViews.kt). P4 priority for the Android app is to adopt compose/user-avatar, compose/user-name, and compose/user-card, replacing the inline implementations in TimelineScreen.kt. The highest-ROI adoption step is getting the main Android app onto compose/content-core, compose/content-view, compose/user-avatar, and compose/user-card, which would replace ~500 LOC of unmaintained inline code. <!-- [^e7a1d-6] -->

## Registry Backfill and Upstream Candidates

Registry backfill gaps to address include: Compose content-minimal (~150 LOC), Compose login-block (ported from 278 LOC SwiftUI), Compose relay-list (ported from 249 LOC SwiftUI), wiring TUI content files into web registry content.ts, and deciding whether web platform is in scope for v1. Candidates for upstreaming from apps to registry include: content-actions (reply/repost/like/zap row from NoteActionsRow), note-row (user-card + content-view + actions from NoteRowView), and NmpMediaRenderer seam (CompositionLocal-based media extensibility from Android gallery). <!-- [^e7a1d-7] -->

## Registry Site

nmpui.f7z.io hosts the NMP component registry (not chirp). The Vite build for the registry imports Swift files via relative paths (../../../../crates/nmp-cli/registry/swiftui/...) from outside web/registry/, so the project must be built locally where the full repo tree is available. <!-- [^f2fd4-2] -->
