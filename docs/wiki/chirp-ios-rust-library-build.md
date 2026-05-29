---
title: Chirp iOS Rust Library Build — Feature Flags and Linkage
slug: chirp-ios-rust-library-build
summary: Chirp iOS links against libnmp_app_chirp.a; the Rust library requires --features marmot for MLS support and must be built before the Xcode project links.
tags:
  - ios
  - rust
  - build
  - marmot
  - ffi
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
---

# Chirp iOS Rust Library Build — Feature Flags and Linkage

> Chirp iOS links against libnmp_app_chirp.a; the Rust library requires --features marmot for MLS support and must be built before the Xcode project links.

## Library Dependency

Chirp iOS links against `libnmp_app_chirp.a`, a Rust static library produced by the `nmp-app-chirp` crate. The Swift bridge references C FFI symbols exported from this library. The library must be built before the Xcode project can link successfully. [^9a2c7-20]

## Marmot Feature Gate

The `marmot` feature is off by default. The iOS build must include `--features marmot` when building the Rust library. Building without this flag produces a library missing Marmot FFI symbols, causing linker errors. The Marmot feature adds MLS (Message Layer Security) group communication support. [^9a2c7-21]

## Build Command

The Rust library must be built with `cargo build --features marmot` targeting the appropriate iOS architecture before the Xcode build. The Xcode project expects the resulting `libnmp_app_chirp.a` at its expected location. [^9a2c7-22]


Library Dependency

If the linker reports missing symbols, verify the correct crate is being built. The initial build attempted `libnmp_ffi` but the project actually links against `libnmp_app_chirp` — the `nmp-app-chirp` crate in `apps/chirp/nmp-app-chirp/`. [^9a2c7-43]
## See Also
- [[xcodegen-project-regeneration|XcodeGen Project Regeneration — Never Hand-Edit project.pbxproj]] — related guide
- [[chirp-ios-simulator|Chirp iOS Simulator — Dedicated Device and Launch Procedure]] — related guide

