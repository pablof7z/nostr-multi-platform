---
title: Typed FlatBuffers Decoders Must Use Native Platform Bindings — Never a Rust→JSON FFI Hop
slug: nfct-native-decoder-not-ffi
summary: Never introduce a Rust→JSON FFI helper to fill a field in a typed FlatBuffers decoder. Generate native Swift/Kotlin bindings from the schema instead; a JSON hop is slower than the generic path and violates D11.
tags:
  - flatbuffers
  - ffi
  - swift
  - kotlin
  - architecture
  - d11
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:cd331450-f93f-48d0-960e-3c73e927775e
---

# Typed FlatBuffers Decoders Must Use Native Platform Bindings — Never a Rust→JSON FFI Hop

> When filling the `contentTree` field in the typed NOFS decoder, the question arose whether to use a Rust→JSON FFI helper (reusing existing Rust NFCT logic) or generate native Swift/Kotlin FlatBuffers bindings. The correct answer is always native bindings.

## Details

- **Rule:** Never introduce a Rust→JSON FFI helper to fill a field in a typed FlatBuffers decoder. Generate native platform bindings from the FlatBuffers schema instead.
- A typed decode path that re-introduces a JSON serialisation/deserialisation hop is *slower* than the generic path it is meant to replace. It also reintroduces the allocation and parse overhead that the typed path exists to eliminate.
- This pattern violates D11 (the directive against unnecessary cross-boundary serialisation hops in the hot render path).
- The correct approach:
  1. Identify the `.fbs` schema for the field in question.
  2. Run `flatc` with the appropriate language target (`--swift`, `--kotlin`) to generate native bindings.
  3. Wire the generated accessor directly into the typed decoder.
- Reusing Rust logic via FFI is acceptable for *side-effects* (e.g. mutation, network calls) but never for *reading structured data* that already exists in a FlatBuffers buffer accessible to the platform runtime.
- When reviewing typed decoder PRs, flag any import of a JSON bridge or FFI decode helper as a blocking issue.

## See Also
- [[flatbuffers-kotlin-version-pin|flatbuffers kotlin version pin]] — related guide
- [[half-landed-migration-is-not-done|half landed migration is not done]] — related guide
- [[chirp-ffi-boot-and-callback-lifetime|chirp ffi boot and callback lifetime]] — related guide
- [[android-stale-render-model-pre-v80|Stale Generic Render Model Breaks Both Paths — Must Be Updated With Typed Migration]] — related guide

- [flatbuffers-kotlin-version-pin](flatbuffers-kotlin-version-pin)
- [half-landed-migration-is-not-done](half-landed-migration-is-not-done)
- [chirp-ffi-boot-and-callback-lifetime](chirp-ffi-boot-and-callback-lifetime)
