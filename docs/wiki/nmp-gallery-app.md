---
title: NmpGallery App Architecture & Platform Shells
slug: nmp-gallery-app
summary: When merging the new apps/nmp-gallery/android/ module, the legacy android/gallery/ module (applicationId org.nmp.gallery) must be deleted first to avoid applica
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-29
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:a1c3e15c-4d85-4e01-9824-7b7bf6a50c43
  - session:c8c2902c-43a6-4b1c-8215-1732dc266895
  - session:e3b42d41-ffd2-44b3-9e5a-93832feb46e0
  - session:f8eb6e59-19f0-4591-a9b4-47453c051d45
  - session:9de494e6-e783-4785-ae67-1f7014dadd5d
  - session:f5503f3a-d44c-4626-b8de-0492ad1f2a6c
  - session:95a6801d-65a6-481c-985b-4bbe2dbe32c4
  - session:6e8af009-f065-464a-98f1-3ec1ee4ed933
  - session:16ca6097-5734-4d49-8b9a-0a20d42a324b
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# NmpGallery App Architecture & Platform Shells

## Android Migration

The legacy Kotlin/Android gallery app at android/gallery/ (applicationId org.nmp.gallery) renders Nmp registry components for screenshot capture, but currently only renders content components; user profile components (UserCard, UserAvatar, etc.) have not been added yet. The Android gallery app must use `openAuthor` and read profile data from `projections.author_view.profile` in kernel snapshots, not from `claim_profile` or `snapshot.profiles`. The nmp-gallery registry must read from `registryJson()` at startup, not be hardcoded. The nmp-gallery native library (`.so`) must be rebuilt with `cargo ndk` whenever new JNI symbols (like `nativeShowcaseReferencesJson`) are added to avoid `UnsatisfiedLinkError` crashes. Before deleting the legacy android/gallery/ module, an audit must determine whether it or apps/nmp-gallery/android/ is the live build target to avoid breaking the build. When merging the new apps/nmp-gallery/android/ module, the legacy android/gallery/ module must be deleted first to avoid applicationId collision. Android gallery entries must be included in .gitignore.

<!-- citations: [^53838-7] [^a1c3e-2] [^c8c29-4] [^e3b42-1] [^16ca6-7] [^f3d8d-14] -->
## Role & Retention

The nmp-gallery app, its static library, and its TUI are retained as the showcase and screenshot source for nmp gallery across iOS, Kotlin, and TUI platforms. The release manifest (release/nmp-release.toml) must include an entry for nmp-gallery-desktop. The desktop gallery app lives at apps/nmp-gallery/desktop within the workspace. It is an egui/eframe component gallery application. It uses the exact same examples and GalleryData::from_live data path as nmp-gallery-tui. The desktop gallery layout uses a sidebar component registry and detail pane, mirroring the TUI registry sections. The sidebar lists all 15 components from REGISTRY_SECTIONS (User / Content / Embeds & Kinds) with section headers in the TUI blue palette and active-row highlighting. The desktop gallery bridge wraps LiveKernel from nmp_gallery_tui using the same kernel path as the TUI (nmp_app_gallery_register → register_defaults with purplepag.es + relay.primal.net) and uses a Reader thread blocking on Receiver<String> to store the latest JSON snapshot.

<!-- citations: [^f8eb6-2] [^9de49-7] [^f5503-4] [^95a68-2] [^6e8af-5] -->
## Closed Decisions

PD-033-A is re-opened as buildable (2026-05-29, ADR-0039): the push projection seam already satisfies the second-app properties, so no new affordances are required. The thesis is unblocked but not yet demonstrated; the podcast player is the live candidate. [^f8eb6-3]
## See Also

An iOS NmpUserPreview app exists and is used for Nmp component preview screenshots. Its source is ephemeral and not tracked in the repository, meaning it will be lost on system restart.

<!-- citations: [^a1c3e-3] -->
