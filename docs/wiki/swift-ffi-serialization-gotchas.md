---
title: Swift FFI Serialization & Data Conversion Gotchas
slug: swift-ffi-serialization-gotchas
summary: Swift `[(String, String)]` tuples cannot be passed directly to NSJSONSerialization; they must be converted to `[[String]]` before serialization.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-29
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:200932fb-5a92-44e0-8d42-2184d2e69094
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:485a5310-d073-41c9-b230-e6e77926a143
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Swift FFI Serialization & Data Conversion Gotchas

## Serialization of String Tuples

Swift `[(String, String)]` tuples cannot be passed directly to NSJSONSerialization; they must be converted to `[[String]]` before serialization. [^fe79b-14]



The borrowed byte pointer provided to the C-ABI update callback is valid only for the callback's duration, requiring Swift to copy the bytes via `Data(bytes:count:)` before decoding. [^20093-13]
## Field-Name Mapping

The `.convertFromSnakeCase` decoder on the snapshot decoder in `KernelBridge.swift:536` automatically maps snake_case Rust fields to camelCase Swift fields. Because `convertFromSnakeCase` in `KernelBridge.decode()` transforms keys before matching, Stage-1 generated Swift types must NOT have explicit CodingKeys — snake_case rawValues in CodingKeys cause double-transform failures. Additionally, the Swift snake-case conversion preserves leading and trailing underscores while removing underscores between words, matching the Rust serde rename rules and preventing private-field aliasing.

For FlatBuffers-decoded structs, `FlatBufferKeyedContainer` converts all Rust snake_case keys to camelCase at storage time, so Swift structs must use camelCase CodingKeys without snake_case rawValues. The `convertFromSnakeCase` function in `FlatBufferKeyedContainer` only converts keys that contain an underscore; other keys pass through unchanged.

In contrast, `JSONDecoder`-decoded structs (e.g., MarmotBridge, Capabilities) must keep their snake_case CodingKey rawValues because JSONDecoder does not perform automatic camelCase conversion.

<!-- citations: [^12b3f-22] [^1670f-18] [^20093-14] [^37e35-6] [^485a5-7] -->
## CI Round-Trip Testing

CI must include a Swift decoder round-trip test that decodes a captured `KernelEvent::Update` JSON through the exact `KernelBridge` decoder config. The codegen-drift check alone does not catch decode regressions. [^1670f-19]

## FlatBuffers Decode Failure

When a non-optional Swift struct field is absent from FlatBuffers data, `FlatBufferKeyedContainer.decoder(forKey:)` throws `DecodingError.keyNotFound`, causing the entire parent decode to fail and the snapshot to be silently dropped. [^485a5-8]

## Symbol Migration & Deletion

Pull symbols with live Swift callers must not be deleted until the Swift consumer is migrated; instead they are deprecated during the Rust transition and deleted only after Swift reads the push projection. [^4edd4-31]
## See Also

