---
title: Chirp Concept-Reads Registry and Codegen
slug: chirp-concept-reads
topic: app-codegen
summary: Chirp's concept-reads registry is `crates/nmp-app-chirp/concept-reads.json` (iOS, facade=ChirpApp) and `crates/nmp-chirp-android-ffi/concept-reads.json` (Androi
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp Concept-Reads Registry and Codegen

## Concept-Reads Registry

Chirp's concept-reads registry is `crates/nmp-app-chirp/concept-reads.json` (iOS, facade=ChirpApp) and `crates/nmp-chirp-android-ffi/concept-reads.json` (Android, facade=AppHandle), both declaring replies/reactions/reposts with no zaps.

The concept-reads binding codegen (#2899) generates Swift, Rust, and Kotlin from a per-app registry with no concept-dependency leak into the binding layer, enforced by the doctrine ratchet. The codegen emits `ConceptReads.generated.swift` (Swift) and `concept_reads_generated.rs` (Rust) via `nmp gen concept-reads --platform swift/rust`. It hardcodes the `crate::facade` module path, requiring a hand-patch to `crate::app::ChirpApp` when the app's facade lives in `app.rs` rather than `facade.rs` — filed as NMP#3004.

Android's concept-reads Rust half is hand-authored in `uniffi_app_loop/concept_reads.rs` (matching the registry's declared shape) because the codegen has no closure/guarded-accessor mode for `Session::with_app` (a UAF-prevention closure that guards the `NmpApp` pointer behind `AppHandle`), which the direct-accessor emitter cannot target — filed as NMP#3005.

<!-- citations: [^dcc80-625b3] [^dcc80-217c6] [^dcc80-20b35] [^dcc80-eb54c] [^dcc80-3d0b2] [^dcc80-1f135] -->
