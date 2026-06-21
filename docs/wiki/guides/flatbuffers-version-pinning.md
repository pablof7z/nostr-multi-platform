---
title: FlatBuffers Version Pinning
slug: flatbuffers-version-pinning
topic: ffi-runtime
summary: "CI pins the intentionally asymmetric FlatBuffers runtime versions (Rust + Swift: 25.12.19, Web/TypeScript: 25.9.23, Android/Kotlin: 25.2.10) and verifies that e"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-06-18
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edcf5-0586-7960-ba68-0b4e9fb81117
---

# FlatBuffers Version Pinning

## FlatBuffers Version Pinning

CI pins the intentionally asymmetric FlatBuffers runtime versions (Rust + Swift: 25.12.19, Web/TypeScript: 25.9.23, Android/Kotlin: 25.2.10) and verifies that each platform's generated bindings contain the matching FLATBUFFERS_25_2_10() guard call. The deliberately-skewed version pins across Rust/Swift/Web/Android are enforced by ci/check-flatbuffers-version-pins.sh plus the codegen-drift workflow step. The FlatBuffers transport versions are intentionally skewed across platforms until every platform package manager publishes the same FlatBuffers version line.

RELAY_DIAGNOSTICS_SCHEMA_VERSION remains 1 despite field removals, reorders, and additions in the FlatBuffers schema, which is a schema-evolution hazard: old decoders reading new buffers (or vice versa) can silently decode wrong field slots. <!-- [^019ed-143] -->

The iOS test fixture at TypedDiagnosticsLifecycleDecoderTests.swift:271 still constructs removed FlatBuffers fields (shortWireId, stateLabel, consumerCountLabel, eventsRxDisplay, shortUrl, roleLabel, connectionLabel, authLabel, totalEventsDisplay, bytesRxDisplay), which should fail to compile against regenerated bindings or tests the old contract if bindings are not regenerated. <!-- [^019ed-144] -->

<!-- citations: [^37e35-3] [^e4861-14] -->

## P1 Slice Requirements

Every wire-shape change must regenerate ALL golden fixtures (including triplicated Rust .fb.hex + Kotlin POPULATED_HEX + Swift populatedHex) and compile both shells + run decoder/parity tests including nmp-app-chirp before pushing, because CI's cargo test job does not compile apps/* crate tests. PR #1542's nip19 adapter caused expected golden-drift (re-encoding to canonical rust-nostr bech32 with different TLV byte layout) but identical decoded coordinates; the 13 affected wire golden fixtures were regenerated.

<!-- citations: [^11850-97] [^11850-136] [^11850-159] [^11850-189] -->
## Android Wallet Status Binding

Android binds the Rust-computed WalletStatus.is_connected bool instead of deriving from the tone discriminant, so errored wallets now correctly show not-connected (P4 Finding 2). <!-- [^11850-98] -->
