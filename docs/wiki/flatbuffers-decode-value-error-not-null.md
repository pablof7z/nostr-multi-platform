---
title: FlatBuffers decode_value Must Return Errors — Never Degrade to Null
slug: flatbuffers-decode-value-error-not-null
summary: FlatBuffers decode_value must return an error for non-finite float values instead of degrading to null.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-26
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
---

# FlatBuffers decode_value Must Return Errors — Never Degrade to Null

## Error Handling for Non-Finite Floats

FlatBuffers decode_value must return an error for non-finite float values (NaN/Infinity) instead of degrading to null. All host platforms (Web, Android) must throw explicit typed errors matching the hardened Rust decoder contract.

<!-- citations: [^37e35-2] [^e4861-2] -->
## Error Handling for Missing Struct Fields

FlatBuffers decode_value must return an error for missing struct fields (string_value, list, map, map pair value) instead of degrading to null. [^37e35-3]

## Error Handling for Unknown Value Kinds

FlatBuffers decode_value must return an error for unknown value kinds instead of degrading to null. [^37e35-4]

## UpdateFrameDecodeError InvalidValue Variant

UpdateFrameDecodeError includes an InvalidValue variant for value-level decode failures. [^37e35-5]

## 64-bit Integer Precision

The Web decoder returns `BigInt` for 64-bit integer values exceeding `Number.MAX_SAFE_INTEGER` (2⁵³) to prevent silent precision loss, rather than clamping or wrapping with `Number()`. The Android decoder uses `JsonUnquotedLiteral` to preserve full `u64` precision for values exceeding `Long.MAX_VALUE`, instead of clamping to `Long.MAX_VALUE`. [^e4861-3]

## Schema-Version Enforcement

Schema-version enforcement must match the iOS baseline (`KernelBridge.swift:525-528`) on both Android and Web platforms, rejecting mismatched frames at both frame-level and payload-level checks. [^e4861-4]
## See Also

