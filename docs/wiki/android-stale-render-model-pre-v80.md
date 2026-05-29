---
title: Stale Generic Render Model Breaks Both Paths — Must Be Updated With Typed Migration
slug: android-stale-render-model-pre-v80
summary: When completing a platform's typed-path migration, the generic fallback render model must also reflect the current Rust emitter output shape; a stale model breaks both paths simultaneously.
tags:
  - android
  - flatbuffers
  - migration
  - render-model
  - kotlin
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:cd331450-f93f-48d0-960e-3c73e927775e
---

# Stale Generic Render Model Breaks Both Paths — Must Be Updated With Typed Migration

> Android's generic render model `ChirpTimelineSnapshot` still used the pre-V-80 flat `{blocks, cards: [ChirpEventCard]}` shape while the Rust emitter had already migrated to the OP-centric `{cards: [ChirpRootCard{card, attribution}], page}` shape. This caused both the generic and typed paths to fail simultaneously.

## Details

- **Rule:** When completing a platform's typed-path migration, verify that the *generic fallback render model* also reflects the current Rust emitter output shape.
- A stale generic model is not a safe fallback — it is a broken fallback. If the Rust emitter has changed its output shape, the generic decoder will misparse the data regardless of whether the typed path is wired in.
- The migration checklist must include: "Does the generic render model's field layout match the current Rust emitter output?"
- Shape changes in the Rust emitter (e.g. adding `attribution` wrapper, adding `page` field, restructuring card arrays) must be propagated to the generic model in the same PR or a tracked prerequisite, not deferred.
- When the typed path is finally wired in and the generic path is retired, the stale model becomes moot — but until that point it is a live correctness risk.
- Cross-reference the Rust FlatBuffers schema and emitter output against the platform-side model structs as part of every migration review.


### Additional Rule

## Verify OP-Centric Shape Before Assuming iOS/Android Parity

Android's render model lagged iOS during the V-80 OP-feed migration: `ChirpTimelineSnapshot` still used the pre-V-80 shape `{blocks, cards: [ChirpEventCard]}` while the Rust emitter had already moved to the OP-centric shape. When implementing any feature that touches the home-feed render model, explicitly verify that Android's `ChirpTimelineSnapshot`/`ChirpOpFeedSnapshot` is on the current OP-centric shape (`{cards:[ChirpRootCard{card,attribution}], page}`) before assuming parity with iOS.
## See Also
- [[half-landed-migration-is-not-done|half landed migration is not done]] — related guide
- [[flatbuffers-kotlin-version-pin|flatbuffers kotlin version pin]] — related guide
- [[nfct-native-decoder-not-ffi|nfct native decoder not ffi]] — related guide
- [[half-landed-migration-is-not-done|half landed migration is not done]] — related guide
- [[nfct-native-decoder-not-ffi|nfct native decoder not ffi]] — related guide

- [half-landed-migration-is-not-done](half-landed-migration-is-not-done)
- [flatbuffers-kotlin-version-pin](flatbuffers-kotlin-version-pin)
- [nfct-native-decoder-not-ffi](nfct-native-decoder-not-ffi)
