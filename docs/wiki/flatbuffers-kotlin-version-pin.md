---
title: Android FlatBuffers Kotlin Bindings Must Match Pinned flatbuffers-java Runtime
slug: flatbuffers-kotlin-version-pin
summary: "Android is pinned to flatbuffers-java:25.2.10; generated Kotlin bindings from newer flatc must be patched to use the FLATBUFFERS_25_2_10() compatibility macro."
tags:
  - android
  - flatbuffers
  - kotlin
  - codegen
  - config
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:322d163a-59eb-4c02-8604-009b4ae4d9b0
  - session:cd331450-f93f-48d0-960e-3c73e927775e
---

# Android FlatBuffers Kotlin Bindings Must Match Pinned flatbuffers-java Runtime

> Android targets in this project are pinned to `flatbuffers-java:25.2.10`. When FlatBuffers Kotlin bindings are regenerated using a newer version of `flatc`, the generated files must be patched to use the `FLATBUFFERS_25_2_10()` compatibility macro so they remain compatible with the pinned runtime. Mismatches cause runtime crashes or silent data corruption.

## Details

- **Gradle pin**: The Android Gradle files declare `implementation("com.google.flatbuffers:flatbuffers-java:25.2.10")` (or equivalent). This version must not be bumped without a coordinated update of all generated bindings.
- **flatc version drift**: The `flatc` compiler used for codegen may be newer than the pinned runtime (e.g., `flatc 25.12.19` generating bindings for a `25.2.10` runtime). This is expected but requires a manual patch step.
- **Compatibility macro**: Generated Kotlin files contain a version macro call (e.g., `FLATBUFFERS_25_12_19()`). This must be changed to `FLATBUFFERS_25_2_10()` to match the runtime. Verify this in every generated file after regeneration.
- **Verification checklist after regeneration**:
  1. Search all generated `.kt` files for the version macro.
  2. Confirm every macro matches `FLATBUFFERS_25_2_10()` (or whatever the current Gradle pin is).
  3. Run Android unit tests and instrumented tests to catch ABI mismatches early.
- **Upgrade path**: To upgrade the runtime, update the Gradle pin first, then regenerate bindings with a matching `flatc` version, then remove the patch step.


### Additional Rule

## flatc/Runtime Version Mismatch Detail

The pinned flatc binary (e.g. 25.12.19) may differ from the pinned flatbuffers-java runtime (e.g. 25.2.10). Generated Kotlin files contain a version guard macro that must match the **runtime**, not the compiler. When generating bindings with a mismatched flatc, patch the version guard macro in every generated file to match the runtime version — check existing generated files in the repo for the established pattern.

## CI Coverage for Gradle Version Pins

`ci/check-flatbuffers-version-pins.sh` must cover **every** gradle file that pins a FlatBuffers version, not just the ones present when the script was first written. When adding a new FlatBuffers dependency to any gradle file (e.g. `android/app/build.gradle.kts` vs. the gallery's `build.gradle`), immediately add a corresponding `require_line` entry to the CI script. Gaps between the active app's pin and the gallery's pin have caused unguarded divergence in the past.

### Additional Rule

## flatc Binary Version vs. Runtime Pin Mismatch

When generating Android/Kotlin FlatBuffers bindings, the available `flatc` binary version (e.g., 25.12.19) will often be newer than the pinned `flatbuffers-java` runtime (currently **25.2.10**). Always patch the version guard macro in generated files to match the pinned runtime version — use `FLATBUFFERS_25_2_10()` — not the flatc binary version. Check existing generated files in `nmp/nip01/` as the canonical reference pattern for how this patching is done.
## See Also
- [[android-ci-pin-gap-app-vs-gallery|android ci pin gap app vs gallery]] — related guide
- [[nfct-native-decoder-not-ffi|Typed FlatBuffers Decoders Must Use Native Platform Bindings — Never a Rust→JSON FFI Hop]] — related guide
- [[android-stale-render-model-pre-v80|Stale Generic Render Model Breaks Both Paths — Must Be Updated With Typed Migration]] — related guide
- [[android-ci-pin-gap-app-vs-gallery|android ci pin gap app vs gallery]] — related guide
- [[flatbuffers-kotlin-version-pin|flatbuffers kotlin version pin]] — related guide
