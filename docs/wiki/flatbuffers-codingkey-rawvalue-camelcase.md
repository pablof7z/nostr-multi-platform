---
title: FlatBuffers CodingKey rawValues Must Be camelCase — convertFromSnakeCase Mismatch
slug: flatbuffers-codingkey-rawvalue-camelcase
summary: Struct CodingKey rawValues must match the camelCase output of convertFromSnakeCase — use \"primaryId\" not \"primary_id\", otherwise every card decode fails with keyNotFound.
tags:
  - flatbuffers
  - swift
  - ios
  - decoding
  - codingkeys
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-27
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:200932fb-5a92-44e0-8d42-2184d2e69094
  - session:54fc9b94-b995-46c6-8372-59c4abe0f95a
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:485a5310-d073-41c9-b230-e6e77926a143
---

# FlatBuffers CodingKey rawValues Must Be camelCase — convertFromSnakeCase Mismatch

> Struct CodingKey rawValues must match the camelCase output of convertFromSnakeCase — use \"primaryId\" not \"primary_id\", otherwise every card decode fails with keyNotFound.

## The convertFromSnakeCase / CodingKey Mismatch

The `FlatBufferKeyedContainer` converts all Rust snake_case keys to camelCase at storage time before looking them up in the CodingKeys dictionary. This transforms snake_case keys to camelCase: `primary_id` becomes `primaryId`, `event_kind` becomes `eventKind`. Swift `convertFromSnakeCase` must preserve leading and trailing underscores while removing underscores between words, matching the Rust serde behavior so future private-looking fields cannot alias public names. If a CodingKey enum case has `rawValue: "primary_id"` (snake_case), the lookup misses the already-converted `"primaryId"` key, producing a `keyNotFound` decode error and an empty feed on every card. All Rust keys converted to camelCase by `FlatBufferKeyedContainer` must have corresponding `CodingKeys` with camelCase rawValues. Stage-1 generated Swift types must not emit explicit CodingKeys because KernelBridge.decode() uses `keyDecodingStrategy = .convertFromSnakeCase`, which causes a double-transform failure with snake_case rawValues. JSONDecoder-decoded structs (MarmotBridge, Capabilities) must keep snake_case CodingKey rawValues because JSONDecoder does not perform automatic camelCase conversion.

<!-- citations: [^4edd4-6] [^4edd4-221] [^1670f-6] [^20093-5] [^37e35-1] [^485a5-4] -->
## Required Pattern

Struct field CodingKey rawValues must use the camelCase form matching the `convertFromSnakeCase` output. For example, `case primaryId = "primaryId"` (NOT `"primary_id"`). This applies to struct field CodingKeys only — enum string VALUES (e.g., `depthLimit`, `unresolvedUri`) correctly remain as snake_case because they are not map keys undergoing conversion. Specific instances: ModulePayload uses `hasGap` (not `has_gap`); ChirpEventCard.CodingKeys uses camelCase case names without explicit snake_case rawValues (authorPubkey, createdAt, contentTree, relationCounts, authorDisplayName, authorPictureUrl, contentPreview); TimelineWindowCursor.CodingKeys uses `createdAt` without the explicit `= "created_at"` rawValue; TimelineWindowPage.CodingKeys uses camelCase case names without snake_case rawValues (nextCursor, hasMore, totalBlocks); NostrWireUri.CodingKeys and NostrWireNode.CodingKeys use camelCase case names without snake_case rawValues (mediaKind, orderedStart). Capabilities structs use JSONDecoder directly (not FlatBuffers), so their snake_case CodingKey rawValues are correct and must not be changed.

<!-- citations: [^4edd4-7] [^485a5-5] -->
## Specific Fix — NostrWireUri

The `NostrWireUri` struct had `CodingKeys.primaryId` with rawValue `"primary_id"` and `CodingKeys.eventKind` with rawValue `"event_kind"`. Both were corrected to `"primaryId"` and `"eventKind"` respectively to match the camelCase output of `convertFromSnakeCase`. [^4edd4-8]


These two fixes — the Rust `claim_sub_index` panic fix and the FlatBuffers `primaryId`/`eventKind` CodingKey fix — were delivered together as PR #810. They were discovered as the two root causes preventing Chirp iOS from booting past the "Background service stopped" crash banner. The Rust panic fix also benefits Android and TUI since it lives in `nmp-core`. [^4edd4-31]
## Platform-Specific Impact

This bug only affects platforms that use the `FlatBufferKeyedContainer` with `convertFromSnakeCase` for map-key decoding. The Android main app uses the FlatBuffers typed path with camelCase field names natively — no fix needed. The Android gallery module uses standard `kotlinx.serialization` JSON (not the FB camelCase converter), so its `@SerialName("primary_id")` is correct. The project memory explicitly warns about this mismatch. [^4edd4-9]

## Symptom in Logs

When this bug is present, the kernel logs show `keyNotFound 'primary_id'` FlatBuffers decode failures on every card in the feed. The typed home-feed decoder fails and falls back to the generic (empty) decode, resulting in a feed with no rendered content despite the kernel being alive and emitting snapshots. KernelBridge.decodeFlatBuffer logs specific missing key names and coding paths on `DecodingError.keyNotFound` and `typeMismatch` for diagnostics. A login dispatch that produces a kernel snapshot where ChirpTimelineSnapshot fails to decode will silently drop the entire snapshot, preventing `activeAccount` from being set and leaving the app stuck on the onboarding screen.

<!-- citations: [^4edd4-10] [^485a5-6] -->
## CI Coverage Gap — Codegen-Drift Workflow

The codegen-drift CI workflow only checks that generated files match a fresh codegen run and does not decode a real envelope through the iOS decoder, allowing regressions like the CodingKeys issue to slip through. A Swift decoder regression CI gate test must be added that decodes a captured KernelEvent::Update JSON through KernelBridge's exact decoder configuration. [^1670f-7]

## UpdateFrameDecodeError — No Silent Degradation to Null

Decoding a FlatBuffers update frame returns an `UpdateFrameDecodeError::InvalidValue` for non-finite float values, missing string/map/list payloads, map pairs missing values, and unknown value kinds, rather than silently degrading to null. [^54fc9-4]
## See Also
- [[flatbuffers-typed-transport|FlatBuffers Typed Transport — Hybrid Migration Architecture]] — related guide
- [[chirp-ios-kernel-types-generated|Chirp iOS KernelTypes.generated.swift — Dev-Time Generation, Lives in Git]] — related guide
- [[chirp-cross-platform-feature-parity-testing|Chirp Cross-Platform Feature Parity — Mandated Testing Across All Clients]] — related guide
- [[chirp-ios-embed-system-implementation|Chirp iOS Embed System — Implementation and Architecture]] — related guide
- [[android-relays-tab-duplicate-key-crash|Android Relays Tab — Duplicate Key Crash in LazyColumn]] — related guide

